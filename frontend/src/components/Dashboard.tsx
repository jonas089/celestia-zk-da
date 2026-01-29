import { useState, useEffect } from 'react';
import api from '../api';
import type { RootResponse, SyncStatusResponse, HealthResponse, HistoryEntry } from '../api';

function Dashboard() {
  const [health, setHealth] = useState<HealthResponse | null>(null);
  const [root, setRoot] = useState<RootResponse | null>(null);
  const [status, setStatus] = useState<SyncStatusResponse | null>(null);
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const fetchData = async () => {
    try {
      setError(null);
      const [healthRes, rootRes, statusRes, historyRes] = await Promise.all([
        api.health(),
        api.getLatestRoot(),
        api.getSyncStatus(),
        api.getHistory(),
      ]);
      setHealth(healthRes);
      setRoot(rootRes);
      setStatus(statusRes);
      setHistory(historyRes.entries);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to connect to node');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchData();
    const interval = setInterval(fetchData, 5000);
    return () => clearInterval(interval);
  }, []);

  if (loading) {
    return <div className="loading">Connecting to node...</div>;
  }

  if (error) {
    return (
      <div className="error-container">
        <div className="error-message">
          <h3>Connection Error</h3>
          <p>{error}</p>
          <p className="hint">Make sure the app node is running on port 16000</p>
          <button onClick={fetchData}>Retry</button>
        </div>
      </div>
    );
  }

  return (
    <div className="dashboard">
      <div className="stats-grid">
        <div className="stat-card">
          <div className="stat-label">Node Status</div>
          <div className={`stat-value status-${health?.status}`}>
            {health?.status === 'ok' ? 'Online' : 'Offline'}
          </div>
          <div className="stat-detail">v{health?.version}</div>
        </div>

        <div className="stat-card">
          <div className="stat-label">Transitions</div>
          <div className="stat-value">{root?.transition_index ?? 0}</div>
          <div className="stat-detail">Total state transitions</div>
        </div>

        <div className="stat-card">
          <div className="stat-label">Celestia</div>
          <div className={`stat-value ${status?.celestia_enabled ? 'enabled' : 'disabled'}`}>
            {status?.celestia_enabled ? 'Enabled' : 'Disabled'}
          </div>
          <div className="stat-detail">
            {status?.last_celestia_height
              ? `Last height: ${status.last_celestia_height}`
              : 'No blobs posted'}
          </div>
        </div>

        <div className="stat-card">
          <div className="stat-label">Proving</div>
          <div className="stat-value enabled">Active</div>
          <div className="stat-detail">SP1 zkVM</div>
        </div>
      </div>

      <div className="section">
        <h2>Current State Root</h2>
        <div className="root-display">
          <code>{root?.root}</code>
        </div>
      </div>

      <div className="section">
        <h2>State History</h2>
        <div className="history-table-container">
          <table className="history-table">
            <thead>
              <tr>
                <th>Sequence</th>
                <th>State Root</th>
                <th>Celestia Height</th>
              </tr>
            </thead>
            <tbody>
              {history.slice().reverse().map((entry) => (
                <tr key={entry.sequence}>
                  <td className="sequence">#{entry.sequence}</td>
                  <td className="root">
                    <code>{entry.root.slice(0, 16)}...{entry.root.slice(-16)}</code>
                  </td>
                  <td className="height">
                    {entry.celestia_height !== null ? (
                      <span className="celestia-badge">{entry.celestia_height}</span>
                    ) : (
                      <span className="pending">Local only</span>
                    )}
                  </td>
                </tr>
              ))}
              {history.length === 0 && (
                <tr>
                  <td colSpan={3} className="empty">No transitions yet</td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}

export default Dashboard;
