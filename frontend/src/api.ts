// API client for app_da_node

const BASE_URL = 'http://127.0.0.1:16000';

export interface RootResponse {
  root: string;
  transition_index: number;
  celestia_height: number | null;
}

export interface MerkleProofResponse {
  key_hash: string;
  value: string | null;
  siblings: string[];
}

export interface ValueResponse {
  key: string;
  value: string | null;
  root: string;
  proof: MerkleProofResponse;
}

export interface SyncStatusResponse {
  transition_index: number;
  latest_root: string;
  celestia_enabled: boolean;
  last_celestia_height: number | null;
}

export interface HistoryEntry {
  sequence: number;
  root: string;
  celestia_height: number | null;
}

export interface HistoryResponse {
  entries: HistoryEntry[];
}

export interface TransitionResponse {
  sequence: number;
  prev_root: string;
  new_root: string;
  public_inputs: string;
  proof: string;
  proof_size_bytes: number;
  program_hash: string;
  celestia_height: number;
}

export interface TransitionsResponse {
  transitions: TransitionResponse[];
}

export interface HealthResponse {
  status: string;
  version: string;
}

export interface ApplyTransitionResponse {
  sequence: number;
  prev_root: string;
  new_root: string;
  celestia_height: number | null;
  proof_size_bytes: number;
}

export interface OperationRequest {
  type: string;
  key: string;
  value?: string;
  encoding?: string;
}

export interface VerifiableOperationRequest {
  op_type: object;
  key: string;
  old_value?: string;
  new_value?: string;
  witness_index: number;
}

export interface ApplyTransitionRequest {
  operations: OperationRequest[];
  public_inputs?: string;
  private_inputs?: string;
  verifiable_operations: VerifiableOperationRequest[];
}

// Account state structure
export interface AccountState {
  balance: number;
  nonce: number;
}

class ApiClient {
  private baseUrl: string;

  constructor(baseUrl: string = BASE_URL) {
    this.baseUrl = baseUrl;
  }

  async health(): Promise<HealthResponse> {
    const res = await fetch(`${this.baseUrl}/health`);
    if (!res.ok) throw new Error(`Health check failed: ${res.status}`);
    return res.json();
  }

  async getLatestRoot(): Promise<RootResponse> {
    const res = await fetch(`${this.baseUrl}/root/latest`);
    if (!res.ok) throw new Error(`Failed to get latest root: ${res.status}`);
    return res.json();
  }

  async getValue(key: string, encoding: 'utf8' | 'hex' = 'utf8'): Promise<ValueResponse> {
    const params = new URLSearchParams({ key, encoding });
    const res = await fetch(`${this.baseUrl}/value?${params}`);
    if (!res.ok) throw new Error(`Failed to get value: ${res.status}`);
    return res.json();
  }

  async getMerkleProof(key: string, encoding: 'utf8' | 'hex' = 'utf8'): Promise<MerkleProofResponse> {
    const params = new URLSearchParams({ key, encoding });
    const res = await fetch(`${this.baseUrl}/proof/merkle?${params}`);
    if (!res.ok) throw new Error(`Failed to get proof: ${res.status}`);
    return res.json();
  }

  async getSyncStatus(): Promise<SyncStatusResponse> {
    const res = await fetch(`${this.baseUrl}/sync/status`);
    if (!res.ok) throw new Error(`Failed to get sync status: ${res.status}`);
    return res.json();
  }

  async getHistory(): Promise<HistoryResponse> {
    const res = await fetch(`${this.baseUrl}/history`);
    if (!res.ok) throw new Error(`Failed to get history: ${res.status}`);
    return res.json();
  }

  async getCelestiaTransition(height: number): Promise<TransitionResponse> {
    const params = new URLSearchParams({ height: height.toString() });
    const res = await fetch(`${this.baseUrl}/celestia/transition?${params}`);
    if (!res.ok) throw new Error(`Failed to get transition: ${res.status}`);
    return res.json();
  }

  async getCelestiaTransitions(fromHeight: number, toHeight: number): Promise<TransitionsResponse> {
    const params = new URLSearchParams({
      from_height: fromHeight.toString(),
      to_height: toHeight.toString(),
    });
    const res = await fetch(`${this.baseUrl}/celestia/transitions?${params}`);
    if (!res.ok) throw new Error(`Failed to get transitions: ${res.status}`);
    return res.json();
  }

  async applyTransition(request: ApplyTransitionRequest): Promise<ApplyTransitionResponse> {
    const res = await fetch(`${this.baseUrl}/transition`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(request),
    });
    if (!res.ok) {
      const err = await res.json().catch(() => ({ error: 'Unknown error' }));
      throw new Error(err.error || `Failed to apply transition: ${res.status}`);
    }
    return res.json();
  }

  // Helper: Get account balance
  async getAccount(name: string): Promise<{ state: AccountState | null; proof: MerkleProofResponse; root: string }> {
    const key = `account:${name}`;
    const response = await this.getValue(key);

    let state: AccountState | null = null;
    if (response.value) {
      // Decode base64 and parse as little-endian u64 values
      const bytes = Uint8Array.from(atob(response.value), c => c.charCodeAt(0));
      if (bytes.length >= 16) {
        const view = new DataView(bytes.buffer);
        state = {
          balance: Number(view.getBigUint64(0, true)),
          nonce: Number(view.getBigUint64(8, true)),
        };
      }
    }

    return { state, proof: response.proof, root: response.root };
  }

  // Helper: Create account
  async createAccount(name: string, initialBalance: number): Promise<ApplyTransitionResponse> {
    const key = `account:${name}`;

    // Encode balance + nonce as little-endian bytes
    const bytes = new Uint8Array(16);
    const view = new DataView(bytes.buffer);
    view.setBigUint64(0, BigInt(initialBalance), true);
    view.setBigUint64(8, BigInt(0), true); // nonce = 0
    const value = btoa(String.fromCharCode(...bytes));

    return this.applyTransition({
      operations: [{ type: 'insert', key, value }],
      verifiable_operations: [{
        op_type: { CreateAccount: { initial_balance: initialBalance } },
        key,
        old_value: undefined,
        new_value: value,
        witness_index: 0,
      }],
    });
  }

  // Helper: Transfer between accounts
  async transfer(from: string, to: string, amount: number): Promise<ApplyTransitionResponse> {
    // First, get current account states
    const fromAccount = await this.getAccount(from);
    const toAccount = await this.getAccount(to);

    if (!fromAccount.state) {
      throw new Error(`Sender account "${from}" does not exist`);
    }

    if (fromAccount.state.balance < amount) {
      throw new Error(`Insufficient balance: ${fromAccount.state.balance} < ${amount}`);
    }

    // Calculate new states
    const newFromBalance = fromAccount.state.balance - amount;
    const newFromNonce = fromAccount.state.nonce + 1;
    const newToBalance = (toAccount.state?.balance ?? 0) + amount;
    const newToNonce = toAccount.state?.nonce ?? 0;

    // Encode new states
    const encodeState = (balance: number, nonce: number): string => {
      const bytes = new Uint8Array(16);
      const view = new DataView(bytes.buffer);
      view.setBigUint64(0, BigInt(balance), true);
      view.setBigUint64(8, BigInt(nonce), true);
      return btoa(String.fromCharCode(...bytes));
    };

    const fromKey = `account:${from}`;
    const toKey = `account:${to}`;
    const newFromValue = encodeState(newFromBalance, newFromNonce);
    const newToValue = encodeState(newToBalance, newToNonce);

    const operations: OperationRequest[] = [
      { type: 'insert', key: fromKey, value: newFromValue },
      { type: 'insert', key: toKey, value: newToValue },
    ];

    const verifiableOperations: VerifiableOperationRequest[] = [
      {
        op_type: { Transfer: { from: fromKey, to: toKey, amount } },
        key: fromKey,
        old_value: fromAccount.proof.value ?? undefined,
        new_value: newFromValue,
        witness_index: 0,
      },
      {
        op_type: { Transfer: { from: fromKey, to: toKey, amount } },
        key: toKey,
        old_value: toAccount.proof.value ?? undefined,
        new_value: newToValue,
        witness_index: 1,
      },
    ];

    return this.applyTransition({
      operations,
      verifiable_operations: verifiableOperations,
    });
  }
}

export const api = new ApiClient();
export default api;
