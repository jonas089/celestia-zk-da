import { useState } from 'react';
import api from '../api';
import type { ApplyTransitionResponse } from '../api';

interface TransferRecord {
  id: number;
  from: string;
  to: string;
  amount: number;
  sequence: number;
  celestiaHeight: number | null;
  proofSize: number;
  timestamp: Date;
}

function Transfers() {
  const [from, setFrom] = useState('');
  const [to, setTo] = useState('');
  const [amount, setAmount] = useState('');
  const [transferring, setTransferring] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [transfers, setTransfers] = useState<TransferRecord[]>([]);
  const [nextId, setNextId] = useState(1);

  const executeTransfer = async () => {
    if (!from.trim() || !to.trim() || !amount.trim()) return;

    const amountNum = parseInt(amount, 10);
    if (isNaN(amountNum) || amountNum <= 0) {
      setError('Amount must be a positive number');
      return;
    }

    setTransferring(true);
    setError(null);

    try {
      const result: ApplyTransitionResponse = await api.transfer(from.trim(), to.trim(), amountNum);

      const record: TransferRecord = {
        id: nextId,
        from: from.trim(),
        to: to.trim(),
        amount: amountNum,
        sequence: result.sequence,
        celestiaHeight: result.celestia_height,
        proofSize: result.proof_size_bytes,
        timestamp: new Date(),
      };

      setTransfers([record, ...transfers]);
      setNextId(nextId + 1);
      setFrom('');
      setTo('');
      setAmount('');
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Transfer failed');
    } finally {
      setTransferring(false);
    }
  };

  const formatTime = (date: Date) => {
    return date.toLocaleTimeString();
  };

  return (
    <div className="transfers">
      <div className="section">
        <h2>New Transfer</h2>
        <div className="transfer-form">
          <div className="form-group">
            <label>From Account</label>
            <input
              type="text"
              placeholder="Sender account name"
              value={from}
              onChange={(e) => setFrom(e.target.value)}
              disabled={transferring}
            />
          </div>
          <div className="form-group">
            <label>To Account</label>
            <input
              type="text"
              placeholder="Recipient account name"
              value={to}
              onChange={(e) => setTo(e.target.value)}
              disabled={transferring}
            />
          </div>
          <div className="form-group">
            <label>Amount</label>
            <input
              type="number"
              placeholder="Amount to transfer"
              value={amount}
              onChange={(e) => setAmount(e.target.value)}
              disabled={transferring}
              min="1"
            />
          </div>
          <button
            className="transfer-btn"
            onClick={executeTransfer}
            disabled={transferring || !from.trim() || !to.trim() || !amount.trim()}
          >
            {transferring ? 'Processing...' : 'Execute Transfer'}
          </button>
        </div>

        {transferring && (
          <div className="proving-indicator">
            <div className="spinner"></div>
            <span>Generating ZK proof for transfer... This may take a moment.</span>
          </div>
        )}

        {error && <div className="error">{error}</div>}
      </div>

      <div className="section">
        <h2>Transfer History</h2>
        {transfers.length === 0 ? (
          <p className="empty-state">No transfers yet. Execute a transfer above.</p>
        ) : (
          <div className="transfers-list">
            {transfers.map((transfer) => (
              <div key={transfer.id} className="transfer-card">
                <div className="transfer-main">
                  <div className="transfer-parties">
                    <span className="party from">{transfer.from}</span>
                    <span className="arrow">→</span>
                    <span className="party to">{transfer.to}</span>
                  </div>
                  <div className="transfer-amount">
                    {transfer.amount.toLocaleString()}
                  </div>
                </div>
                <div className="transfer-meta">
                  <span className="meta-item">
                    <span className="label">Transition:</span>
                    <span className="value">#{transfer.sequence}</span>
                  </span>
                  <span className="meta-item">
                    <span className="label">Proof:</span>
                    <span className="value">{(transfer.proofSize / 1024).toFixed(1)} KB</span>
                  </span>
                  {transfer.celestiaHeight !== null && (
                    <span className="meta-item celestia">
                      <span className="label">Celestia:</span>
                      <span className="value">{transfer.celestiaHeight}</span>
                    </span>
                  )}
                  <span className="meta-item time">
                    {formatTime(transfer.timestamp)}
                  </span>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      <div className="section info-section">
        <h2>How Transfers Work</h2>
        <div className="info-content">
          <ol>
            <li><strong>State Read:</strong> Current balances and nonces are fetched with Merkle proofs</li>
            <li><strong>Validation:</strong> Sender balance is checked, new states are computed</li>
            <li><strong>ZK Proof:</strong> SP1 generates a proof that the transition is valid</li>
            <li><strong>State Commit:</strong> New Merkle root is computed and stored</li>
            <li><strong>DA Post:</strong> Proof blob is posted to Celestia for permanent availability</li>
          </ol>
          <p className="note">
            Anyone can verify the proof chain by fetching blobs from Celestia and checking each SP1 proof.
          </p>
        </div>
      </div>
    </div>
  );
}

export default Transfers;
