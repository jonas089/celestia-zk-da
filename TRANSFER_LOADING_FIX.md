# Transfer Transaction UI Loading Fix

## Problem Summary

Transfer transactions in the UI were displaying "Loading transition data..." but never showing the actual proof details, while account creation transactions worked correctly.

## Root Cause

**Timing Issue with Celestia Blob Propagation:**

1. **Transfer transactions are more complex:**
   - Modify 2 accounts (sender + receiver)
   - Generate 2 witness paths
   - Create larger SP1 proofs (~5-10 seconds)
   - Take longer to post to Celestia

2. **Race condition:**
   - Backend posts blob to Celestia and immediately adds entry to `root_history`
   - Frontend refreshes history and sees new entry with `celestia_height`
   - User clicks on entry → frontend immediately queries Celestia for blob
   - **Celestia hasn't finished propagating the blob yet** → returns "blob: not found"
   - API returns 404 NOT_FOUND
   - Frontend catches error but leaves UI in empty state

3. **Account creation worked because:**
   - Simpler proofs (1 account, 1 witness)
   - Faster generation (~2-3 seconds)
   - By the time user clicks, blob is usually available

## Solution Implemented

### 1. Retry Logic with Exponential Backoff

Added `fetchTransitionWithRetry()` function that:
- Attempts to fetch the transition data up to 5 times
- Uses exponential backoff: 1s, 2s, 4s, 8s, 16s between attempts
- Total retry window: ~31 seconds (sufficient for blob propagation)
- If all retries fail, displays error message with manual retry option

### 2. Error State Management

- Added `transitionError` state variable
- Displays user-friendly error message explaining the situation
- Provides a "Retry" button for manual retry
- Error clears automatically when retry succeeds

### 3. UI Improvements

- Error message with context: "Transition data not yet available on Celestia. The proof may still be propagating."
- Styled error box with retry button
- Maintains loading state during retries so user knows something is happening

## Files Modified

1. **[ProofExplorer.tsx](frontend/src/components/ProofExplorer.tsx)**
   - Added `transitionError` state
   - Implemented `fetchTransitionWithRetry()` with exponential backoff
   - Added `retryFetchTransition()` for manual retry
   - Updated UI to show error state and retry button

2. **[App.css](frontend/src/App.css)**
   - Added `.error-inline` styles
   - Added `.retry-btn` styles within error context

## Testing

To verify the fix:

1. Start the node and UI: `make start`
2. Create a transfer transaction (which takes longer to prove)
3. Immediately click on the new transition in the history list
4. Observe:
   - Loading indicator appears
   - If blob isn't ready, automatic retries occur (check browser console)
   - If all retries fail, error message appears with retry button
   - Clicking retry button fetches again
   - Once blob is available, proof details display correctly

## Technical Details

### Retry Sequence
```
Attempt 1: immediate
Attempt 2: wait 1s
Attempt 3: wait 2s  (total: 3s)
Attempt 4: wait 4s  (total: 7s)
Attempt 5: wait 8s  (total: 15s)
Attempt 6: wait 16s (total: 31s)
```

This gives Celestia sufficient time to propagate the blob while providing quick success for faster proofs.

### Error Recovery
- User can manually retry at any time
- Retries clear previous error state
- Selecting a different transition resets all state
- No permanent failures - always recoverable

## Future Improvements (Optional)

1. **Polling indicator:** Show "Retry X of 5" during automatic retries
2. **WebSocket updates:** Backend pushes notification when blob is confirmed on Celestia
3. **Optimistic UI:** Show partial data (like public inputs) from local state before Celestia confirmation
4. **Backend-side retry:** Have the API endpoint poll Celestia before responding to frontend

## Why This Approach

- **Non-breaking:** Doesn't change backend behavior or data flow
- **Resilient:** Handles transient Celestia propagation delays gracefully
- **User-friendly:** Clear error messages and manual recovery option
- **Efficient:** Exponential backoff prevents API spam while ensuring timely success
- **Debuggable:** Console logs show retry attempts for troubleshooting
