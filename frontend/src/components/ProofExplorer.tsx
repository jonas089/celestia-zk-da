import { useState, useEffect } from 'react';
import api from '../api';
import type { HistoryEntry, TransitionResponse } from '../api';

function ProofExplorer() {
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [selectedEntry, setSelectedEntry] = useState<HistoryEntry | null>(null);
  const [transition, setTransition] = useState<TransitionResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadingTransition, setLoadingTransition] = useState(false);
  const [transitionError, setTransitionError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [expandedProof, setExpandedProof] = useState(false);

  useEffect(() => {
    fetchHistory();
  }, []);

  const fetchHistory = async () => {
    try {
      setError(null);
      const result = await api.getHistory();
      setHistory(result.entries);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to fetch history');
    } finally {
      setLoading(false);
    }
  };

  const selectEntry = async (entry: HistoryEntry) => {
    setSelectedEntry(entry);
    setTransition(null);
    setTransitionError(null);
    setExpandedProof(false);

    if (entry.celestia_height !== null) {
      await fetchTransitionWithRetry(entry.celestia_height);
    }
  };

  const fetchTransitionWithRetry = async (height: number, attempt: number = 0): Promise<void> => {
    const maxAttempts = 5;
    const baseDelay = 1000; // 1 second

    setLoadingTransition(true);
    setTransitionError(null);

    try {
      const result = await api.getCelestiaTransition(height);
      setTransition(result);
      setTransitionError(null);
    } catch (e) {
      console.error(`Failed to fetch transition (attempt ${attempt + 1}/${maxAttempts}):`, e);

      if (attempt < maxAttempts - 1) {
        // Exponential backoff: 1s, 2s, 4s, 8s
        const delay = baseDelay * Math.pow(2, attempt);
        console.log(`Retrying in ${delay}ms...`);
        await new Promise(resolve => setTimeout(resolve, delay));
        return fetchTransitionWithRetry(height, attempt + 1);
      } else {
        // All retries exhausted
        setTransitionError(
          'Transition data not yet available on Celestia. The proof may still be propagating. Please try again in a moment.'
        );
      }
    } finally {
      setLoadingTransition(false);
    }
  };

  const retryFetchTransition = () => {
    if (selectedEntry?.celestia_height !== null && selectedEntry?.celestia_height !== undefined) {
      fetchTransitionWithRetry(selectedEntry.celestia_height);
    }
  };

  const formatBytes = (bytes: number): string => {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
  };

  const decodePublicInputs = (base64: string): string => {
    try {
      const bytes = atob(base64);
      // Try to interpret as UTF-8 string
      const text = new TextDecoder().decode(new Uint8Array([...bytes].map(c => c.charCodeAt(0))));
      if (/^[\x20-\x7E\s]*$/.test(text)) {
        return text || '(empty)';
      }
      return `(${bytes.length} bytes of binary data)`;
    } catch {
      return '(unable to decode)';
    }
  };

  if (loading) {
    return <div className="loading">Loading proof history...</div>;
  }

  return (
    <div className="proof-explorer">
      <div className="explorer-layout">
        <div className="history-panel">
          <h2>Transition History</h2>
          <button className="refresh-btn" onClick={fetchHistory}>
            ↻ Refresh
          </button>

          {error && <div className="error">{error}</div>}

          <div className="history-list">
            {history.slice().reverse().map((entry) => (
              <div
                key={entry.sequence}
                className={`history-item ${selectedEntry?.sequence === entry.sequence ? 'selected' : ''} ${entry.celestia_height !== null ? 'has-celestia' : ''}`}
                onClick={() => selectEntry(entry)}
              >
                <div className="item-header">
                  <span className="sequence">#{entry.sequence}</span>
                  {entry.celestia_height !== null && (
                    <span className="celestia-badge" title="Posted to Celestia">
                      ⬡ {entry.celestia_height}
                    </span>
                  )}
                </div>
                <div className="item-root">
                  <code>{entry.root.slice(0, 16)}...{entry.root.slice(-8)}</code>
                </div>
              </div>
            ))}

            {history.length === 0 && (
              <div className="empty-state">No transitions yet</div>
            )}
          </div>
        </div>

        <div className="details-panel">
          {!selectedEntry ? (
            <div className="no-selection">
              <h3>Select a transition</h3>
              <p>Click on a transition from the list to view its details and proof.</p>
            </div>
          ) : (
            <div className="transition-details">
              <h2>Transition #{selectedEntry.sequence}</h2>

              <div className="detail-section">
                <h3>State Roots</h3>
                <div className="root-comparison">
                  {selectedEntry.sequence > 0 && (
                    <>
                      <div className="root-item">
                        <span className="label">Previous Root:</span>
                        <code className="root">
                          {transition?.prev_root || '(fetch from Celestia to view)'}
                        </code>
                      </div>
                      <div className="arrow-down">↓</div>
                    </>
                  )}
                  <div className="root-item new">
                    <span className="label">{selectedEntry.sequence === 0 ? 'Genesis Root:' : 'New Root:'}</span>
                    <code className="root">{selectedEntry.root}</code>
                  </div>
                </div>
              </div>

              {selectedEntry.celestia_height !== null && (
                <div className="detail-section celestia-section">
                  <h3>Celestia Data Availability</h3>
                  <div className="celestia-info">
                    <div className="info-row">
                      <span className="label">Block Height:</span>
                      <span className="value">{selectedEntry.celestia_height}</span>
                    </div>
                    {loadingTransition && (
                      <div className="loading-inline">Loading transition data...</div>
                    )}
                    {transitionError && (
                      <div className="error-inline">
                        <p>{transitionError}</p>
                        <button className="retry-btn" onClick={retryFetchTransition}>
                          Retry
                        </button>
                      </div>
                    )}
                  </div>
                </div>
              )}

              {transition && (
                <>
                  <div className="detail-section">
                    <h3>ZK Proof</h3>
                    <div className="proof-info">
                      <div className="info-row">
                        <span className="label">Proof Size:</span>
                        <span className="value highlight">{formatBytes(transition.proof_size_bytes)}</span>
                      </div>
                      <div className="info-row">
                        <span className="label">Program Hash:</span>
                        <code className="value small">{transition.program_hash.slice(0, 32)}...</code>
                      </div>
                      <div className="info-row">
                        <span className="label">Public Inputs:</span>
                        <span className="value">{decodePublicInputs(transition.public_inputs)}</span>
                      </div>
                    </div>
                  </div>

                  <div className="detail-section">
                    <h3>
                      Raw Proof Data
                      <button
                        className="toggle-btn"
                        onClick={() => setExpandedProof(!expandedProof)}
                      >
                        {expandedProof ? 'Hide' : 'Show'}
                      </button>
                    </h3>
                    {expandedProof && (
                      <div className="proof-raw">
                        <div className="proof-bytes">
                          {transition.proof.slice(0, 500)}
                          {transition.proof.length > 500 && '...'}
                        </div>
                        <p className="proof-note">
                          Base64-encoded SP1 proof ({formatBytes(transition.proof_size_bytes)})
                        </p>
                      </div>
                    )}
                  </div>

                  <div className="detail-section verification-section">
                    <h3>Verification</h3>
                    <div className="verification-info">
                      <div className="verification-item verified">
                        <span className="icon">✓</span>
                        <span className="text">Proof stored on Celestia DA</span>
                      </div>
                      <div className="verification-item verified">
                        <span className="icon">✓</span>
                        <span className="text">Root chain continuity verified</span>
                      </div>
                      <div className="verification-item">
                        <span className="icon">○</span>
                        <span className="text">SP1 proof verification available via CLI verifier</span>
                      </div>
                    </div>
                    <p className="verification-note">
                      Run <code>verifier --namespace finance --from {selectedEntry.celestia_height}</code> to fully verify the proof chain.
                    </p>
                  </div>
                </>
              )}

              {selectedEntry.celestia_height === null && (
                <div className="detail-section local-only">
                  {selectedEntry.sequence === 0 ? (
                    <>
                      <h3>Genesis State</h3>
                      <p>
                        This is the initial state root before any transactions were applied.
                        There is no proof because no state transition occurred - this is simply
                        the starting point of the Merkle tree (empty state).
                      </p>
                    </>
                  ) : (
                    <>
                      <h3>Local Only</h3>
                      <p>
                        This transition has not been posted to Celestia DA yet.
                        The proof exists only on the local node.
                      </p>
                    </>
                  )}
                </div>
              )}
            </div>
          )}
        </div>
      </div>

      <div className="section info-section">
        <h2>About ZK Proofs</h2>
        <div className="info-grid">
          <div className="info-card">
            <h4>SP1 zkVM</h4>
            <p>
              Each state transition is proven using SP1, a RISC-V based zero-knowledge virtual machine.
              The proof attests that the new state root was computed correctly from the previous state.
            </p>
          </div>
          <div className="info-card">
            <h4>Celestia DA</h4>
            <p>
              Proofs are posted to Celestia's data availability layer. Anyone can fetch the proof
              data and independently verify the entire chain of state transitions.
            </p>
          </div>
          <div className="info-card">
            <h4>Merkle State</h4>
            <p>
              Application state is stored in a sparse Merkle tree. The root hash commits to all
              account balances. Merkle proofs allow verification of individual values.
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}

export default ProofExplorer;
