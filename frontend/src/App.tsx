import { useState } from 'react';
import Dashboard from './components/Dashboard';
import Accounts from './components/Accounts';
import Transfers from './components/Transfers';
import ProofExplorer from './components/ProofExplorer';
import './App.css';

type Tab = 'dashboard' | 'accounts' | 'transfers' | 'proofs';

function App() {
  const [activeTab, setActiveTab] = useState<Tab>('dashboard');

  return (
    <div className="app">
      <header className="header">
        <h1>ZK Finance Explorer</h1>
        <p className="subtitle">Zero-Knowledge Proof Explorer for Celestia DA</p>
      </header>

      <nav className="nav">
        <button
          className={activeTab === 'dashboard' ? 'active' : ''}
          onClick={() => setActiveTab('dashboard')}
        >
          Dashboard
        </button>
        <button
          className={activeTab === 'accounts' ? 'active' : ''}
          onClick={() => setActiveTab('accounts')}
        >
          Accounts
        </button>
        <button
          className={activeTab === 'transfers' ? 'active' : ''}
          onClick={() => setActiveTab('transfers')}
        >
          Transfers
        </button>
        <button
          className={activeTab === 'proofs' ? 'active' : ''}
          onClick={() => setActiveTab('proofs')}
        >
          Proof Explorer
        </button>
      </nav>

      <main className="main">
        {activeTab === 'dashboard' && <Dashboard />}
        {activeTab === 'accounts' && <Accounts />}
        {activeTab === 'transfers' && <Transfers />}
        {activeTab === 'proofs' && <ProofExplorer />}
      </main>

      <footer className="footer">
        <p>Powered by SP1 ZK Proofs &bull; Celestia DA</p>
      </footer>
    </div>
  );
}

export default App;
