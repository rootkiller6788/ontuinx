#!/bin/bash
# Three pipelines independently call the SAME LLM with the SAME prompt.
# Each generates code → verifies → reports what it found.
set -e
P="tests/real/comparison/prompt.txt"
M="${LLM_MODEL:-deepseek-chat}"
K="${DEEPSEEK_API_KEY:-$ANTHROPIC_API_KEY}"
[ -z "$K" ] && { echo "Set DEEPSEEK_API_KEY or ANTHROPIC_API_KEY"; exit 1; }

call_llm() {
  # Returns: generated C code on stdout
  if [ -n "$DEEPSEEK_API_KEY" ]; then
    curl -s https://api.deepseek.com/v1/chat/completions \
      -H "Content-Type: application/json" \
      -H "Authorization: Bearer $DEEPSEEK_API_KEY" \
      -d "{\"model\":\"$M\",\"messages\":[{\"role\":\"user\",\"content\":$(cat "$P" | jq -Rs .)}],\"max_tokens\":8192,\"temperature\":0.2}" \
      | jq -r '.choices[0].message.content // empty'
  else
    curl -s https://api.anthropic.com/v1/messages \
      -H "Content-Type: application/json" \
      -H "x-api-key: $ANTHROPIC_API_KEY" \
      -H "anthropic-version: 2023-06-01" \
      -d "{\"model\":\"$M\",\"max_tokens\":8192,\"messages\":[{\"role\":\"user\",\"content\":$(cat "$P" | jq -Rs .)}]}" \
      | jq -r '.content[0].text // empty'
  fi
}

D="/tmp/onto_compare_$$"; mkdir -p "$D"
echo "═══════════════════════════════════════════════════════════════"
echo "  Each pipeline independently calls $M"
echo "  Same prompt: $(wc -w < "$P") words"
echo "═══════════════════════════════════════════════════════════════"

# ── Pipeline ①: OpenCode ──
echo ""; echo "─── ① OpenCode: calling LLM → gcc -Wall -Wextra ───"
G1="$D/opencode.c"
call_llm > "$G1" 2>/dev/null
head -1 "$G1" | grep -q '```' && { sed -n '/```c\|```/,/```/p' "$G1" | grep -v '```' > "$G1.tmp"; mv "$G1.tmp" "$G1"; }
echo "  Generated: $(wc -l < "$G1") lines"
gcc -Wall -Wextra -Werror -fopenmp -O3 -o /dev/null "$G1" >"$D/o1.log" 2>&1 || true
O1=$(grep -cE 'warning:|error:' "$D/o1.log" 2>/dev/null || echo 0)
echo "  OpenCode found: $O1 issues"

# ── Pipeline ②: OntoRuntime ──
echo ""; echo "─── ② OntoRuntime: calling LLM → +ASan +Valgrind ───"
G2="$D/runtime.c"
call_llm > "$G2" 2>/dev/null
head -1 "$G2" | grep -q '```' && { sed -n '/```c\|```/,/```/p' "$G2" | grep -v '```' > "$G2.tmp"; mv "$G2.tmp" "$G2"; }
echo "  Generated: $(wc -l < "$G2") lines"
gcc -Wall -fopenmp -O2 -g -fsanitize=address,undefined -o "$D/rt" "$G2" >"$D/o2.log" 2>&1 || true
if [ -x "$D/rt" ]; then
  timeout 10 "$D/rt" >/dev/null 2>>"$D/o2.log" || true
fi
command -v valgrind >/dev/null 2>&1 && valgrind --leak-check=full "$D/rt" >>"$D/o2.log" 2>&1 || true
O2=$(grep -cE 'ERROR|AddressSanitizer|UndefinedBehavior|definitely lost|indirectly lost|error:|undefined reference' "$D/o2.log" 2>/dev/null || echo 0)
echo "  OntoRuntime found: $O2 issues"

# ── Pipeline ③: OntoRuntime+OntoAssure ──
echo ""; echo "─── ③ OntoRuntime+Assure: calling LLM → +Verdict +Energy ───"
G3="$D/assure.c"
call_llm > "$G3" 2>/dev/null
head -1 "$G3" | grep -q '```' && { sed -n '/```c\|```/,/```/p' "$G3" | grep -v '```' > "$G3.tmp"; mv "$G3.tmp" "$G3"; }
echo "  Generated: $(wc -l < "$G3") lines"
gcc -Wall -Wextra -fopenmp -O2 -g -fsanitize=address,undefined -o "$D/as" "$G3" >"$D/o3.log" 2>&1 || true
gcc -Wall -Wextra -Werror -fopenmp -O3 -o /dev/null "$G3" >"$D/o3w.log" 2>&1 || true
if [ -x "$D/as" ]; then
  timeout 10 "$D/as" > "$D/energy.csv" 2>>"$D/o3.log" || true
  [ -s "$D/energy.csv" ] && {
    F=$(head -1 "$D/energy.csv" | cut -d, -f2 2>/dev/null)
    L=$(tail -1 "$D/energy.csv" | cut -d, -f2 2>/dev/null)
    [ -n "$F" ] && [ -n "$L" ] && [ "$F" != "0" ] && {
      DRIFT=$(python3 -c "print(f'{abs($L-$F)/abs($F)*100:.4f}%')" 2>/dev/null || echo "N/A")
      echo "[Assure/energy] drift=$DRIFT" >> "$D/o3.log"
    }
  }
fi
command -v valgrind >/dev/null 2>&1 && valgrind --leak-check=full "$D/as" >>"$D/o3.log" 2>&1 || true
O3W=$(grep -cE 'warning:|error:' "$D/o3w.log" 2>/dev/null || echo 0)
O3R=$(grep -cE 'ERROR|AddressSanitizer|UndefinedBehavior|definitely lost|indirectly lost' "$D/o3.log" 2>/dev/null || echo 0)
O3=$((O3W + O3R))
echo "  OntoRuntime+Assure found: $O3 issues ($O3W warnings + $O3R runtime)"

# ── RESULTS ──
echo ""
echo "══════════════════════════════════════"
echo "  Each pipeline independently called $M"
echo "  Same prompt → independent code generation → independent verification"
echo "══════════════════════════════════════"
printf "  %-30s %4s %4s %4s\n" "Pipeline" "Lin" "Iss" "Δ"
printf "  %-30s %4s %4s %4s\n" "──────────────────────────────" "───" "───" "───"
printf "  %-30s %4d %4d %4s\n" "① OpenCode (gcc)" "$(wc -l < "$G1")" "$O1" "—"
printf "  %-30s %4d %4d %+4d\n" "② OntoRuntime (+ASan)" "$(wc -l < "$G2")" "$O2" "$((O2 - O1))"
printf "  %-30s %4d %4d %+4d\n" "③ OntoRuntime+Assure" "$(wc -l < "$G3")" "$O3" "$((O3 - O1))"
echo ""
echo "  Logs: $D/"
