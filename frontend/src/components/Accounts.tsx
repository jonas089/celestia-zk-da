import { useState } from 'react';
import api from '../api';
import type { AccountState, MerkleProofResponse } from '../api';

interface AccountData {
  name: string;
  state: AccountState | null;
  proof: MerkleProofResponse;
  root: string;
}

function Accounts() {
  const [accounts, setAccounts] = useState<AccountData[]>([]);
  const [lookupName, setLookupName] = useState('');
  const [createName, setCreateName] = useState('');
  const [createBalance, setCreateBalance] = useState('1000');
  const [loading, setLoading] = useState(false);
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);
  const [selectedProof, setSelectedProof] = useState<AccountData | null>(null);

  const lookupAccount = async () => {
    if (!lookupName.trim()) return;

    setLoading(true);
    setError(null);

    try {
      const result = await api.getAccount(lookupName.trim());
      const existing = accounts.find(a => a.name === lookupName.trim());
      if (existing) {
        setAccounts(accounts.map(a =>
          a.name === lookupName.trim()
            ? { ...a, state: result.state, proof: result.proof, root: result.root }
            : a
        ));
      } else {
        setAccounts([...accounts, {
          name: lookupName.trim(),
          state: result.state,
          proof: result.proof,
          root: result.root,
        }]);
      }
      setLookupName('');
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to lookup account');
    } finally {
      setLoading(false);
    }
  };

  const createAccount = async () => {
    if (!createName.trim()) return;

    setCreating(true);
    setError(null);
    setSuccess(null);

    try {
      const balance = parseInt(createBalance, 10);
      if (isNaN(balance) || balance < 0) {
        throw new Error('Invalid balance');
      }

      const result = await api.createAccount(createName.trim(), balance);
      setSuccess(`Account "${createName}" created! Transition #${result.sequence}, Proof: ${result.proof_size_bytes.toLocaleString()} bytes`);
      setCreateName('');
      setCreateBalance('1000');

      // Auto-lookup the new account
      const accountData = await api.getAccount(createName.trim());
      setAccounts([...accounts, {
        name: createName.trim(),
        state: accountData.state,
        proof: accountData.proof,
        root: accountData.root,
      }]);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to create account');
    } finally {
      setCreating(false);
    }
  };

  const refreshAccount = async (name: string) => {
    try {
      const result = await api.getAccount(name);
      setAccounts(accounts.map(a =>
        a.name === name
          ? { ...a, state: result.state, proof: result.proof, root: result.root }
          : a
      ));
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to refresh account');
    }
  };

  const removeAccount = (name: string) => {
    setAccounts(accounts.filter(a => a.name !== name));
    if (selectedProof?.name === name) {
      setSelectedProof(null);
    }
  };

  return (
    <div className="accounts">
      <div className="section">
        <h2>Create Account</h2>
        <div className="form-row">
          <input
            type="text"
            placeholder="Account name"
            value={createName}
            onChange={(e) => setCreateName(e.target.value)}
            disabled={creating}
          />
          <input
            type="number"
            placeholder="Initial balance"
            value={createBalance}
            onChange={(e) => setCreateBalance(e.target.value)}
            disabled={creating}
            min="0"
          />
          <button onClick={createAccount} disabled={creating || !createName.trim()}>
            {creating ? 'Creating...' : 'Create Account'}
          </button>
        </div>
        {creating && (
          <div className="proving-indicator">
            <div className="spinner"></div>
            <span>Generating ZK proof... This may take a moment.</span>
          </div>
        )}
      </div>

      <div className="section">
        <h2>Lookup Account</h2>
        <div className="form-row">
          <input
            type="text"
            placeholder="Account name to lookup"
            value={lookupName}
            onChange={(e) => setLookupName(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && lookupAccount()}
            disabled={loading}
          />
          <button onClick={lookupAccount} disabled={loading || !lookupName.trim()}>
            {loading ? 'Looking up...' : 'Lookup'}
          </button>
        </div>
      </div>

      {error && <div className="error">{error}</div>}
      {success && <div className="success">{success}</div>}

      <div className="section">
        <h2>Tracked Accounts</h2>
        {accounts.length === 0 ? (
          <p className="empty-state">No accounts tracked. Create or lookup an account above.</p>
        ) : (
          <div className="accounts-grid">
            {accounts.map((account) => (
              <div key={account.name} className={`account-card ${account.state ? 'exists' : 'not-found'}`}>
                <div className="account-header">
                  <h3>{account.name}</h3>
                  <div className="account-actions">
                    <button className="icon-btn" onClick={() => refreshAccount(account.name)} title="Refresh">
                      ↻
                    </button>
                    <button className="icon-btn" onClick={() => removeAccount(account.name)} title="Remove">
                      ×
                    </button>
                  </div>
                </div>

                {account.state ? (
                  <div className="account-body">
                    <div className="account-stat">
                      <span className="label">Balance</span>
                      <span className="value">{account.state.balance.toLocaleString()}</span>
                    </div>
                    <div className="account-stat">
                      <span className="label">Nonce</span>
                      <span className="value">{account.state.nonce}</span>
                    </div>
                  </div>
                ) : (
                  <div className="account-body">
                    <p className="not-found-message">Account not found</p>
                  </div>
                )}

                <div className="account-footer">
                  <button
                    className="proof-btn"
                    onClick={() => setSelectedProof(selectedProof?.name === account.name ? null : account)}
                  >
                    {selectedProof?.name === account.name ? 'Hide Proof' : 'View Merkle Proof'}
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      {selectedProof && (
        <div className="section proof-section">
          <h2>Merkle Proof for "{selectedProof.name}"</h2>
          <div className="proof-details">
            <div className="proof-item">
              <span className="label">Key Hash:</span>
              <code>{selectedProof.proof.key_hash}</code>
            </div>
            <div className="proof-item">
              <span className="label">Value (base64):</span>
              <code>{selectedProof.proof.value || 'null (not found)'}</code>
            </div>
            <div className="proof-item">
              <span className="label">State Root:</span>
              <code>{selectedProof.root}</code>
            </div>
            <div className="proof-item">
              <span className="label">Proof Path ({selectedProof.proof.siblings.length} siblings):</span>
            </div>
            <div className="siblings-list">
              {selectedProof.proof.siblings.slice(0, 10).map((sibling, i) => (
                <div key={i} className="sibling">
                  <span className="index">{i}:</span>
                  <code>{sibling.slice(0, 32)}...</code>
                </div>
              ))}
              {selectedProof.proof.siblings.length > 10 && (
                <div className="sibling more">
                  ... and {selectedProof.proof.siblings.length - 10} more siblings
                </div>
              )}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

export default Accounts;
