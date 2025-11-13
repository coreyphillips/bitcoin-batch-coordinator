import axios from 'axios';

const API_BASE_URL = import.meta.env.VITE_API_URL || 'http://localhost:3000/api/v1';

export const api = axios.create({
  baseURL: API_BASE_URL,
  headers: {
    'Content-Type': 'application/json',
  },
});

// Types
export interface Batch {
  id: string;
  intent_data: string;
  created_at: number;
  started_at?: number;
  completed_at?: number;
  state: 'filling' | 'ready' | 'signing' | 'completed' | 'failed';
  txid?: string;
  total_fees?: number;
  participant_count: number;
  raw_transaction?: string;
}

export interface BatchParticipant {
  id: number;
  batch_id: string;
  pubkey: string;
  joined_at: number;
  commitment?: string;
  reveal?: string;
  signed: boolean;
}

export interface Stats {
  date: string;
  batches_completed: number;
  total_participants: number;
  total_fees_saved: number;
}

export interface StatsResponse {
  daily_stats: Stats[];
  total_batches: number;
  total_participants: number;
  total_fees_saved: number;
}

// API Functions
export const fetchBatches = async (state?: string): Promise<{ batches: Batch[]; total: number }> => {
  const params = state ? { state } : {};
  const response = await api.get('/batches', { params });
  return response.data;
};

export const fetchBatch = async (id: string): Promise<Batch> => {
  const response = await api.get(`/batches/${id}`);
  return response.data;
};

export const fetchStats = async (days: number = 30): Promise<StatsResponse> => {
  const response = await api.get('/stats', { params: { days } });
  return response.data;
};

export const fetchHistory = async (limit: number = 100): Promise<{ batches: Batch[]; total: number }> => {
  const response = await api.get('/history', { params: { limit } });
  return response.data;
};

export const createBatch = async (params: {
  min_participants: number;
  max_participants: number;
  timeout_seconds: number;
}): Promise<{ id: string; message: string }> => {
  const response = await api.post('/batches', params);
  return response.data;
};
