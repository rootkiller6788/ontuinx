/*
 * pass_c_fnptr_bindings.c — Static C function pointer binding extraction.
 *
 * Extracts function-pointer-to-function bindings from C static initializers:
 *   1. Struct callback tables:  struct ops x = { .read = my_read, ... };
 *   2. Function pointer arrays: void (*h[])(int) = { func_a, func_b };
 *   3. Single fnptr init:       void (*h)(int) = &func_a;
 *
 * Creates CallbackSlot nodes and POINTS_TO edges in the graph buffer so
 * indirect-call paths are traversable without dataflow analysis.
 *
 * Coverage status is logged per-file: COMPLETE / PARTIAL / UNSUPPORTED /
 * PARSE_FAILED / SOURCE_UNAVAILABLE / AMBIGUOUS_TARGET.  The pass is
 * soft-failure by design — errors are logged, never fatal.
 *
 * Edge model:
 *   Variable/File  --DEFINES_CALLBACK_SLOT-->  CallbackSlot  --POINTS_TO-->  Function
 *
 * CallbackSlot QN:  <module_qn>.__callback_slot__.<table>.<field_or_index>
 */
#include "foundation/constants.h"
#include "pipeline/pipeline.h"
#include "pipeline/pipeline_internal.h"
#include "graph_buffer/graph_buffer.h"
#include "foundation/log.h"
#include "foundation/compat.h"
#include "foundation/compat_fs.h"
#include "foundation/limits.h"
#include "cbm.h"
#include "helpers.h" /* cbm_find_child_by_kind, CBM_DECLARATOR_DEPTH_LIMIT */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* ── Constants ───────────────────────────────────────────────────── */

/* Max AST walk depth before bailing (defensive). */
enum { FNPTR_MAX_DEPTH = 512 };

/* Edge / node label strings. */
#define EDGE_POINTS_TO            "POINTS_TO"
#define EDGE_DEFINES_CB_SLOT     "DEFINES_CALLBACK_SLOT"
#define LABEL_CALLBACK_SLOT       "CallbackSlot"
#define CB_SLOT_SEP              ".__callback_slot__."

/* ── Forward declarations ────────────────────────────────────────── */

static int scan_c_file(cbm_pipeline_ctx_t *ctx, const char *file_path, const char *source,
                       int source_len, TSTree *tree, const char *module_qn, int *detected,
                       int *unresolved, int *ambig);

/* ── Helpers ─────────────────────────────────────────────────────── */

static char *read_file(const char *path, int *out_len) {
    FILE *f = cbm_fopen(path, "rb");
    if (!f) return NULL;
    (void)fseek(f, 0, SEEK_END);
    long size = ftell(f);
    (void)fseek(f, 0, SEEK_SET);
    if (size <= 0 || size > cbm_max_file_bytes()) {
        (void)fclose(f);
        return NULL;
    }
    enum { TS_PAD = 16 };
    char *buf = malloc((size_t)size + TS_PAD);
    if (!buf) { (void)fclose(f); return NULL; }
    size_t nread = fread(buf, SKIP_ONE, size, f);
    (void)fclose(f);
    memset(buf + nread, 0, TS_PAD);
    *out_len = (int)nread;
    return buf;
}

static const char *itoa_log(int val) {
    enum { N = 4, M = 3 };
    static CBM_TLS char bufs[N][CBM_SZ_32];
    static CBM_TLS int idx = 0;
    int i = idx; idx = (idx + SKIP_ONE) & M;
    snprintf(bufs[i], sizeof(bufs[i]), "%d", val);
    return bufs[i];
}

/* Extract source text of a node. Returns a malloc'd copy. */
static char *node_text_owned(TSNode node, const char *source) {
    uint32_t start = ts_node_start_byte(node);
    uint32_t end = ts_node_end_byte(node);
    if (end <= start) return NULL;
    size_t len = end - start;
    char *s = malloc(len + SKIP_ONE);
    if (!s) return NULL;
    memcpy(s, source + start, len);
    s[len] = '\0';
    return s;
}

/* Resolve an identifier against the graph buffer and registry.
 * Tries (in order):
 *   1. gbuf QN lookup: <module_qn>.<name>
 *   2. gbuf by-name lookup (simple name match)
 *   3. registry resolve
 * Returns the target Function/Method node, or NULL. */
static const cbm_gbuf_node_t *resolve_target(cbm_pipeline_ctx_t *ctx, const char *name,
                                             const char *module_qn) {
    if (!name || !name[0] || !module_qn) return NULL;

    /* 1. Qualified-name lookup in gbuf. */
    char qn_buf[CBM_SZ_512];
    int slen = snprintf(qn_buf, sizeof(qn_buf), "%s.%s", module_qn, name);
    if (slen > 0 && slen < (int)sizeof(qn_buf)) {
        const cbm_gbuf_node_t *n = cbm_gbuf_find_by_qn(ctx->gbuf, qn_buf);
        if (n) return n;
    }

    /* 2. Simple-name match in gbuf (find Function/Method with that name). */
    const cbm_gbuf_node_t **candidates = NULL;
    int cand_count = 0;
    if (cbm_gbuf_find_by_name(ctx->gbuf, name, &candidates, &cand_count) == 0 && cand_count > 0) {
        /* Prefer one in the same file. */
        const cbm_gbuf_node_t *best = NULL;
        for (int i = 0; i < cand_count; i++) {
            const char *lbl = candidates[i]->label;
            if (!lbl) continue;
            if (strcmp(lbl, "Function") == 0 || strcmp(lbl, "Method") == 0) {
                if (!best) best = candidates[i];
            }
        }
        if (best) return best;
        /* Fall back to any candidate. */
        return candidates[0];
    }

    /* 3. Registry lookup (for cross-file resolution). */
    if (ctx->registry) {
        cbm_resolution_t res = cbm_registry_resolve(ctx->registry, name, module_qn, NULL, NULL, 0);
        if (res.qualified_name && res.qualified_name[0]) {
            return cbm_gbuf_find_by_qn(ctx->gbuf, res.qualified_name);
        }
    }

    return NULL;
}

/* Build a CallbackSlot QN: <module_qn>.__callback_slot__.<table>.<slot> */
static char *make_slot_qn(const char *module_qn, const char *table, const char *slot) {
    size_t mlen = module_qn ? strlen(module_qn) : 0;
    size_t tlen = table ? strlen(table) : 0;
    size_t slen = slot ? strlen(slot) : 0;
    size_t sep_len = strlen(CB_SLOT_SEP);
    /* module.__cb_slot__.table.slot\0 */
    size_t total = mlen + sep_len + tlen + SKIP_ONE + slen + SKIP_ONE;
    char *qn = malloc(total);
    if (!qn) return NULL;
    snprintf(qn, total, "%s%s%s.%s", module_qn ? module_qn : "", CB_SLOT_SEP, table, slot);
    return qn;
}

/* Emit one callback binding:
 *   1. Find or create the CallbackSlot node
 *   2. Create DEFINES_CALLBACK_SLOT edge from file node
 *   3. Create POINTS_TO edge from CallbackSlot to target function
 * Returns 1 on success (edge created), 0 on skip, -1 on error. */
static int emit_callback_binding(cbm_pipeline_ctx_t *ctx, const char *module_qn,
                                 const char *file_path, const char *table_name,
                                 const char *slot_field, int slot_index,
                                 const cbm_gbuf_node_t *target, int source_line) {
    if (!ctx || !ctx->gbuf || !target || !file_path) return CBM_NOT_FOUND;

    /* Build slot display name. */
    char slot_name[CBM_SZ_128];
    if (slot_field) {
        snprintf(slot_name, sizeof(slot_name), "%s", slot_field);
    } else {
        snprintf(slot_name, sizeof(slot_name), "%d", slot_index);
    }

    /* Build CallbackSlot QN. */
    char *slot_qn = make_slot_qn(module_qn, table_name, slot_name);
    if (!slot_qn) return CBM_NOT_FOUND;

    /* Build properties JSON for the slot node. */
    char props[CBM_SZ_512];
    const char *field_val = slot_field ? slot_field : "null";
    const char *index_val = slot_index >= 0 ? itoa_log(slot_index) : "null";
    /* Quote the field name if it's a string (not "null"). */
    char quoted_field[CBM_SZ_256];
    if (slot_field) {
        snprintf(quoted_field, sizeof(quoted_field), "\"%s\"", slot_field);
        field_val = quoted_field;
    }
    snprintf(props, sizeof(props),
             "{\"table\":\"%s\",\"field\":%s,\"index\":%s,\"provenance\":\"c_static_initializer\"}",
             table_name, field_val, index_val);

    /* Upsert the CallbackSlot node. */
    int64_t slot_id = cbm_gbuf_upsert_node(ctx->gbuf, LABEL_CALLBACK_SLOT, slot_name, slot_qn,
                                           file_path, source_line, source_line, props);
    if (slot_id <= 0) {
        free(slot_qn);
        return CBM_NOT_FOUND;
    }
    free(slot_qn);

    /* Find the File node to anchor the DEFINES_CALLBACK_SLOT edge.
     * Try the Variable node first (if table_name is a named var), then File node. */
    char file_qn[CBM_SZ_512];
    snprintf(file_qn, sizeof(file_qn), "%s.__file__", module_qn);
    const cbm_gbuf_node_t *file_node = cbm_gbuf_find_by_qn(ctx->gbuf, file_qn);

    /* DEFINES_CALLBACK_SLOT: File → CallbackSlot */
    if (file_node) {
        cbm_gbuf_insert_edge(ctx->gbuf, file_node->id, slot_id, EDGE_DEFINES_CB_SLOT, "{}");
    }

    /* POINTS_TO: CallbackSlot → target Function */
    char edge_props[CBM_SZ_256];
    snprintf(edge_props, sizeof(edge_props),
             "{\"provenance\":\"c_static_initializer\",\"table\":\"%s\",\"slot\":\"%s\"}",
             table_name, slot_name);
    int64_t edge_id =
        cbm_gbuf_insert_edge(ctx->gbuf, slot_id, target->id, EDGE_POINTS_TO, edge_props);

    return edge_id > 0 ? SKIP_ONE : 0;
}

/* ── AST Scanners ────────────────────────────────────────────────── */

/* Check if a node tree contains a function_declarator (indicates function pointer type). */
static bool type_has_function_declarator(TSNode node) {
    /* Walk children of the type part looking for function_declarator. */
    uint32_t nc = ts_node_child_count(node);
    for (uint32_t i = 0; i < nc; i++) {
        TSNode child = ts_node_child(node, i);
        const char *kind = ts_node_type(child);
        if (!kind) continue;
        if (strcmp(kind, "function_declarator") == 0) return true;
        if (strcmp(kind, "pointer_declarator") == 0 ||
            strcmp(kind, "array_declarator") == 0 ||
            strcmp(kind, "parenthesized_declarator") == 0) {
            if (type_has_function_declarator(child)) return true;
        }
    }
    return false;
}

/* Check whether a declaration's declarator chain involves function pointer types.
 * Walks the declaration looking for function_declarator in the declarator subtree. */
static bool is_function_pointer_decl(TSNode decl_node) {
    uint32_t nc = ts_node_child_count(decl_node);
    for (uint32_t i = 0; i < nc; i++) {
        TSNode child = ts_node_child(decl_node, i);
        const char *ck = ts_node_type(child);
        if (!ck) continue;
        /* The type part of the declaration: check children for function_declarator. */
        if (strcmp(ck, "function_declarator") == 0) return true;
        if (type_has_function_declarator(child)) return true;
    }
    return false;
}

/* Scan a struct callback table: look for initializer_lists with field_designator children.
 * Pattern:  struct ops x = { .read = my_func, .write = other_func };
 * AST: declaration → init_declarator → value:initializer_list → pairs of
 *      (field_designator + identifier). */
static int scan_struct_callback_table(cbm_pipeline_ctx_t *ctx, TSNode root, const char *source,
                                      const char *module_qn, const char *file_path, int *detected,
                                      int *unresolved) {
    int found = 0;
    /* Use a cursor for full AST walk. */
    TSTreeCursor cursor = ts_tree_cursor_new(root);
    uint32_t depth = 0;

    for (;;) {
        TSNode node = ts_tree_cursor_current_node(&cursor);
        const char *kind = ts_node_type(node);

        if (strcmp(kind, "initializer_list") == 0) {
            /* Check if this initializer list contains nested pairs.
             * Each pair in a designated initializer looks like:
             *   initializer_list { field_designator, identifier }
             * Iterate children to find nested initializer_list items. */
            uint32_t nc = ts_node_child_count(node);
            TSNode table_name_node = {0};
            /* Walk up to find the containing variable name. */
            {
                TSNode parent = ts_node_parent(node);
                while (!ts_node_is_null(parent)) {
                    const char *pk = ts_node_type(parent);
                    if (strcmp(pk, "init_declarator") == 0) {
                        /* Find the declarator → identifier child. */
                        TSNode decl = cbm_find_child_by_kind(parent, "declarator");
                        if (!ts_node_is_null(decl)) {
                            table_name_node = cbm_find_child_by_kind(decl, "identifier");
                        }
                        if (ts_node_is_null(table_name_node)) {
                            /* Try direct identifier child. */
                            table_name_node = cbm_find_child_by_kind(parent, "identifier");
                        }
                        /* Try pointer_declarator → identifier chain. */
                        if (ts_node_is_null(table_name_node) && !ts_node_is_null(decl)) {
                            TSNode inner = decl;
                            for (int di = 0; di < CBM_DECLARATOR_DEPTH_LIMIT; di++) {
                                TSNode ptr = cbm_find_child_by_kind(inner, "pointer_declarator");
                                if (ts_node_is_null(ptr)) {
                                    ptr = cbm_find_child_by_kind(inner, "array_declarator");
                                }
                                if (!ts_node_is_null(ptr)) {
                                    inner = ptr;
                                    table_name_node = cbm_find_child_by_kind(inner, "identifier");
                                    if (!ts_node_is_null(table_name_node)) break;
                                } else {
                                    table_name_node = cbm_find_child_by_kind(inner, "identifier");
                                    break;
                                }
                            }
                        }
                        break;
                    }
                    if (strcmp(pk, "declaration") == 0 || strcmp(pk, "translation_unit") == 0)
                        break;
                    parent = ts_node_parent(parent);
                }
            }
            char *table_name =
                ts_node_is_null(table_name_node) ? NULL : node_text_owned(table_name_node, source);
            if (!table_name) table_name = strdup("__anon__");

            /* Walk children: each field_designator + value pair. */
            const char *current_field = NULL;
            for (uint32_t i = 0; i < nc; i++) {
                TSNode child = ts_node_child(node, i);
                const char *ck = ts_node_type(child);

                if (strcmp(ck, "field_designator") == 0) {
                    /* Get the field name from its field_identifier child. */
                    TSNode fid = cbm_find_child_by_kind(child, "field_identifier");
                    if (ts_node_is_null(fid)) {
                        /* Some grammars use "identifier" directly. */
                        fid = cbm_find_child_by_kind(child, "identifier");
                    }
                    char *fname = ts_node_is_null(fid) ? NULL : node_text_owned(fid, source);
                    /* Set current_field for the next identifier to pick up. */
                    free((void *)current_field);
                    current_field = fname;
                    /* If this field_designator has a sibling identifier, process it now. */
                } else if (strcmp(ck, "identifier") == 0 && current_field) {
                    /* This identifier is a function name value. */
                    char *func_name = node_text_owned(child, source);
                    if (func_name) {
                        const cbm_gbuf_node_t *target =
                            resolve_target(ctx, func_name, module_qn);
                        if (target) {
                            uint32_t line = ts_node_start_point(child).row + TS_LINE_OFFSET;
                            emit_callback_binding(ctx, module_qn, file_path, table_name,
                                                  current_field, CBM_NOT_FOUND, target, (int)line);
                            (*detected)++;
                            found++;
                        } else {
                            (*unresolved)++;
                        }
                        free(func_name);
                    }
                    free((void *)current_field);
                    current_field = NULL;
                } else if (strcmp(ck, "initializer_list") == 0) {
                    /* Nested initializer list — handle both designated (.field = val) and
                     * positional ({field, val}) patterns. Walk its children. */
                    uint32_t inc = ts_node_child_count(child);
                    const char *nested_field = NULL;
                    for (uint32_t j = 0; j < inc; j++) {
                        TSNode ic = ts_node_child(child, j);
                        const char *ick = ts_node_type(ic);
                        if (strcmp(ick, "field_designator") == 0) {
                            /* Designated: .field = value */
                            TSNode fid = cbm_find_child_by_kind(ic, "field_identifier");
                            if (ts_node_is_null(fid))
                                fid = cbm_find_child_by_kind(ic, "identifier");
                            char *fname =
                                ts_node_is_null(fid) ? NULL : node_text_owned(fid, source);
                            free((void *)nested_field);
                            nested_field = fname;
                        } else if (strcmp(ick, "identifier") == 0 && nested_field) {
                            /* Value identifier following a field_designator. */
                            char *func_name = node_text_owned(ic, source);
                            if (func_name) {
                                const cbm_gbuf_node_t *target =
                                    resolve_target(ctx, func_name, module_qn);
                                if (target) {
                                    uint32_t line =
                                        ts_node_start_point(ic).row + TS_LINE_OFFSET;
                                    emit_callback_binding(ctx, module_qn, file_path,
                                                          table_name, nested_field,
                                                          CBM_NOT_FOUND, target, (int)line);
                                    (*detected)++;
                                    found++;
                                } else {
                                    (*unresolved)++;
                                }
                                free(func_name);
                            }
                            free((void *)nested_field);
                            nested_field = NULL;
                        } else if (strcmp(ick, "string_literal") == 0) {
                            /* Positional: string field name like {"read", func}.
                             * Save as inner_field for positional fallback below. */
                        } else if (strcmp(ick, "identifier") == 0 &&
                                   ts_node_is_null((TSNode){0})) {
                            /* Positional: first identifier could be field name. */
                        }
                    }
                    /* If no designated fields were found, fall back to positional detection. */
                    if (!nested_field) {
                        /* Positional: try {field_name, func_name} or {field_name, func_name, ...} */
                        TSNode inner_field = {0}, inner_val = {0};
                        for (uint32_t j = 0; j < inc; j++) {
                            TSNode ic = ts_node_child(child, j);
                            const char *ick = ts_node_type(ic);
                            if (strcmp(ick, "string_literal") == 0 &&
                                ts_node_is_null(inner_field)) {
                                inner_field = ic;
                            } else if (strcmp(ick, "identifier") == 0) {
                                if (ts_node_is_null(inner_field)) {
                                    inner_field = ic;
                                } else if (ts_node_is_null(inner_val)) {
                                    inner_val = ic;
                                }
                            }
                        }
                        char *pos_field = ts_node_is_null(inner_field)
                                              ? NULL
                                              : node_text_owned(inner_field, source);
                        char *pos_func = ts_node_is_null(inner_val)
                                             ? NULL
                                             : node_text_owned(inner_val, source);
                        if (pos_field && pos_func) {
                            char clean_field[CBM_SZ_128] = {0};
                            const char *fn = pos_field;
                            if (fn[0] == '"') {
                                size_t fl = strlen(fn);
                                if (fl >= CBM_QUOTE_PAIR) {
                                    size_t cl = fl - CBM_QUOTE_PAIR;
                                    if (cl >= sizeof(clean_field))
                                        cl = sizeof(clean_field) - SKIP_ONE;
                                    memcpy(clean_field, fn + SKIP_ONE, cl);
                                    fn = clean_field;
                                }
                            }
                            const cbm_gbuf_node_t *target =
                                resolve_target(ctx, pos_func, module_qn);
                            if (target) {
                                uint32_t line =
                                    ts_node_start_point(inner_val).row + TS_LINE_OFFSET;
                                emit_callback_binding(ctx, module_qn, file_path, table_name,
                                                      fn, CBM_NOT_FOUND, target, (int)line);
                                (*detected)++;
                                found++;
                            } else {
                                (*unresolved)++;
                            }
                        }
                        free(pos_field);
                        free(pos_func);
                    }
                    free((void *)nested_field);
                }
            }
            free((void *)current_field);
            free(table_name);
        }

        /* Depth-first walk. */
        if (ts_tree_cursor_goto_first_child(&cursor)) {
            depth++;
            if (depth > FNPTR_MAX_DEPTH) goto bail;
            continue;
        }
        if (ts_tree_cursor_goto_next_sibling(&cursor)) continue;
        bool up = false;
        while (ts_tree_cursor_goto_parent(&cursor)) {
            depth--;
            if (ts_tree_cursor_goto_next_sibling(&cursor)) { up = true; break; }
        }
        if (!up) break;
    }
bail:
    ts_tree_cursor_delete(&cursor);
    return found;
}

/* Scan function pointer arrays and single fnptr inits.
 * Patterns:
 *   void (*handlers[])(int) = { func_a, func_b, NULL };
 *   void (*handler)(int) = &func_a;
 * Detects via function_declarator presence in the declaration's type subtree. */
static int scan_fnptr_declarations(cbm_pipeline_ctx_t *ctx, TSNode root, const char *source,
                                   const char *module_qn, const char *file_path, int *detected,
                                   int *unresolved) {
    int found = 0;
    TSTreeCursor cursor = ts_tree_cursor_new(root);
    uint32_t depth = 0;

    for (;;) {
        TSNode node = ts_tree_cursor_current_node(&cursor);
        const char *kind = ts_node_type(node);

        if (strcmp(kind, "declaration") == 0 && is_function_pointer_decl(node)) {
            /* This declaration involves function pointers. Find the initializer. */
            TSNode init_decl = cbm_find_child_by_kind(node, "init_declarator");
            if (!ts_node_is_null(init_decl)) {
                /* Get the variable name. */
                TSNode decl_name = {0};
                TSNode decl = cbm_find_child_by_kind(init_decl, "declarator");
                if (!ts_node_is_null(decl)) {
                    decl_name = cbm_find_child_by_kind(decl, "identifier");
                }
                if (ts_node_is_null(decl_name))
                    decl_name = cbm_find_child_by_kind(init_decl, "identifier");

                char *var_name =
                    ts_node_is_null(decl_name) ? strdup("__anon__")
                                               : node_text_owned(decl_name, source);

                /* Find the initializer value. */
                TSNode value = cbm_find_child_by_kind(init_decl, "initializer_list");
                bool is_single = ts_node_is_null(value);
                if (is_single) {
                    /* Single value: identifier, unary_expression, or call_expression. */
                    value = cbm_find_child_by_kind(init_decl, "identifier");
                    if (ts_node_is_null(value))
                        value = cbm_find_child_by_kind(init_decl, "unary_expression");
                }

                if (!ts_node_is_null(value)) {
                    if (strcmp(ts_node_type(value), "initializer_list") == 0) {
                        /* Array of function pointers: extract each identifier child. */
                        uint32_t nc = ts_node_child_count(value);
                        int idx = 0;
                        for (uint32_t i = 0; i < nc; i++) {
                            TSNode child = ts_node_child(value, i);
                            const char *ck = ts_node_type(child);
                            char *func_name = NULL;
                            if (strcmp(ck, "identifier") == 0) {
                                func_name = node_text_owned(child, source);
                            } else if (strcmp(ck, "initializer_list") == 0) {
                                /* Nested — get first identifier. */
                                TSNode id = cbm_find_child_by_kind(child, "identifier");
                                if (!ts_node_is_null(id))
                                    func_name = node_text_owned(id, source);
                            }
                            if (func_name) {
                                /* Skip NULL sentinels. */
                                if (strcmp(func_name, "NULL") != 0 &&
                                    strcmp(func_name, "nullptr") != 0) {
                                    const cbm_gbuf_node_t *target =
                                        resolve_target(ctx, func_name, module_qn);
                                    if (target) {
                                        uint32_t line =
                                            ts_node_start_point(child).row + TS_LINE_OFFSET;
                                        emit_callback_binding(ctx, module_qn, file_path, var_name,
                                                              NULL, idx, target, (int)line);
                                        (*detected)++;
                                        found++;
                                    } else {
                                        (*unresolved)++;
                                    }
                                }
                                free(func_name);
                            }
                            idx++;
                        }
                    } else {
                        /* Single function pointer:  void (*h)(int) = &func; */
                        char *func_name = node_text_owned(value, source);
                        if (func_name) {
                            /* Strip leading '&' if present in unary_expression. */
                            const char *clean = func_name;
                            if (clean[0] == '&') clean++;

                            const cbm_gbuf_node_t *target =
                                resolve_target(ctx, clean, module_qn);
                            if (target) {
                                uint32_t line = ts_node_start_point(value).row + TS_LINE_OFFSET;
                                emit_callback_binding(ctx, module_qn, file_path, var_name, NULL, 0,
                                                      target, (int)line);
                                (*detected)++;
                                found++;
                            } else {
                                (*unresolved)++;
                            }
                            free(func_name);
                        }
                    }
                }
                free(var_name);
            }
        }

        /* Depth-first walk. */
        if (ts_tree_cursor_goto_first_child(&cursor)) {
            depth++;
            if (depth > FNPTR_MAX_DEPTH) goto bail2;
            continue;
        }
        if (ts_tree_cursor_goto_next_sibling(&cursor)) continue;
        bool up = false;
        while (ts_tree_cursor_goto_parent(&cursor)) {
            depth--;
            if (ts_tree_cursor_goto_next_sibling(&cursor)) { up = true; break; }
        }
        if (!up) break;
    }
bail2:
    ts_tree_cursor_delete(&cursor);
    return found;
}

/* ── Per-file scanner ────────────────────────────────────────────── */

static int scan_c_file(cbm_pipeline_ctx_t *ctx, const char *file_path, const char *source,
                       int source_len, TSTree *tree, const char *module_qn, int *detected,
                       int *unresolved, int *ambig) {
    (void)source_len;
    TSNode root = ts_tree_root_node(tree);
    if (ts_node_is_null(root)) return 0;

    int found = 0;
    found += scan_struct_callback_table(ctx, root, source, module_qn, file_path, detected,
                                        unresolved);
    found += scan_fnptr_declarations(ctx, root, source, module_qn, file_path, detected, unresolved);

    (void)ambig; /* reserved for multi-candidate disambiguation in a future version */
    return found;
}

/* ── Main entry point ────────────────────────────────────────────── */

int cbm_pipeline_pass_c_fnptr_bindings(cbm_pipeline_ctx_t *ctx, const cbm_file_info_t *files,
                                       int file_count, CBMFileResult **cache) {
    cbm_log_info("pass.start", "pass", "c_fnptr_bindings", "files", itoa_log(file_count));

    int total_detected = 0, total_unresolved = 0, total_ambig = 0;
    int files_scanned = 0, files_skipped = 0;

    for (int i = 0; i < file_count; i++) {
        if (cbm_pipeline_check_cancel(ctx)) return CBM_NOT_FOUND;

        /* Only C files. */
        if (files[i].language != CBM_LANG_C) continue;

        const char *rel = files[i].rel_path;
        const char *status = "COMPLETE";
        int detected = 0, unresolved = 0, ambig = 0;

        /* ── Obtain AST + source ──
         * Try cached tree + retained source first, then cached tree + re-read,
         * then re-extraction. */
        TSTree *tree = NULL;
        const char *source_ptr = NULL;
        int source_len = 0;
        char *owned_source = NULL;
        CBMFileResult *owned_result = NULL;
        bool must_free_result = false;

        if (cache && cache[i]) {
            tree = cache[i]->cached_tree;
            source_ptr = cache[i]->source;
            source_len = cache[i]->source_len;
        }

        /* If no retained source but we have a cached tree, re-read the file. */
        if (tree && !source_ptr) {
            owned_source = read_file(files[i].path, &source_len);
            if (owned_source) {
                source_ptr = owned_source;
            }
        }

        /* If no tree at all, re-extract. */
        if (!tree) {
            owned_source = read_file(files[i].path, &source_len);
            if (owned_source) {
                owned_result = cbm_extract_file(owned_source, source_len, CBM_LANG_C,
                                                ctx->project_name, rel, CBM_EXTRACT_BUDGET, NULL,
                                                NULL);
                if (owned_result) {
                    tree = owned_result->cached_tree;
                    source_ptr = owned_source;
                    must_free_result = true;
                }
            }
        }

        if (!tree || !source_ptr) {
            cbm_log_info("pass.fnptr.status", "file", rel, "status", "SOURCE_UNAVAILABLE",
                         "detected", "0", "unresolved", "0");
            files_skipped++;
            free(owned_source);
            if (owned_result) cbm_free_result(owned_result);
            continue;
        }

        /* ── Compute module QN ── */
        char *module_qn =
            cbm_pipeline_fqn_module_dir(ctx->project_name, rel, /* C is file-scoped */ false);

        /* ── Scan ── */
        scan_c_file(ctx, files[i].path, source_ptr, source_len, tree, module_qn, &detected,
                    &unresolved, &ambig);

        /* ── Report status ── */
        if (detected == 0 && unresolved == 0) {
            status = "COMPLETE"; /* No fnptr patterns found — nothing to do. */
        } else if (unresolved > 0 && detected == 0) {
            status = "UNSUPPORTED";
        } else if (unresolved > 0) {
            status = "PARTIAL";
        } else {
            status = "COMPLETE";
        }

        cbm_log_info("pass.fnptr.status", "file", rel, "status", status, "detected",
                     itoa_log(detected), "unresolved", itoa_log(unresolved), "ambig",
                     itoa_log(ambig));

        total_detected += detected;
        total_unresolved += unresolved;
        total_ambig += ambig;
        files_scanned++;

        /* Cleanup. */
        free(module_qn);
        free(owned_source);
        if (must_free_result && owned_result) cbm_free_result(owned_result);
    }

    cbm_log_info("pass.done", "pass", "c_fnptr_bindings", "files_scanned", itoa_log(files_scanned),
                 "files_skipped", itoa_log(files_skipped), "detected", itoa_log(total_detected),
                 "unresolved", itoa_log(total_unresolved), "ambig", itoa_log(total_ambig));

    return 0;
}
