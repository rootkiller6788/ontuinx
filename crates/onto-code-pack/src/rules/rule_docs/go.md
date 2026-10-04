# Go Code Review Rules

## Concurrency and Goroutines
- Goroutine leaks: goroutines that never exit or lack cancellation
- Closing channels from the send side only; reading from closed channels
- Missing `WaitGroup.Add(1)` before `go func()` or mismatched `Add`/`Done` counts
- Holding mutexes across channel operations or I/O
- Data races on shared variables without synchronization

## Context Propagation
- `context.Background()` or `context.TODO()` in request-handling paths instead of the request's context
- Context not passed to downstream calls (DB, RPC, HTTP)
- Missing `ctx.Done()` checks in long-running loops
- Context cancelled but work continues without checking `ctx.Err()`
- `context.WithTimeout` or `WithDeadline` without corresponding `defer cancel()`

## Error Handling
- Errors silently ignored: `_ = err` or bare `value, _ := fn()`
- `panic` in library code instead of returning errors
- Errors wrapped with `%v` instead of `%w` (breaking `errors.Is`/`As`)
- Sentinel error comparison without `errors.Is`
- Error messages that lack context: `return err` instead of `fmt.Errorf("doing X: %w", err)`

## Nil Safety
- `nil` map access (read is OK, write panics)
- Typed nil interfaces: `var ptr *T = nil; var iface I = ptr; iface != nil` is true
- Missing nil check after type assertion before using the concrete value
- Nil slice vs empty slice distinction where JSON encoding matters

## Resource Management
- `resp.Body.Close()` not called on HTTP responses (including error paths)
- `sql.Rows` not closed (including after `rows.Err()`)
- `os.File` not closed; `defer` in loop accumulating file descriptors
- `time.Ticker` not stopped
- Database connections, gRPC connections not closed

## Temporal/OntoFlow Specific
- Activity or Workflow code that is non-deterministic (random, time.Now(), map iteration, goroutine)
- Missing idempotency key for critical activities
- Workflow code that blocks on external calls without context deadline
- Missing heartbeat from long-running activities
- Retry policy not configured for transient errors
- Activity input/output types that break backward compatibility
- `workflow.Sleep` used instead of `workflow.NewTimer` with cancellation

## Testing
- Table-driven tests without parallel execution where safe
- Tests using `time.Sleep` instead of synchronization primitives
- Test cleanup not using `t.Cleanup` for resource teardown
- Missing error path test coverage for critical logic
- `os.Getenv` in tests without `t.Setenv`
