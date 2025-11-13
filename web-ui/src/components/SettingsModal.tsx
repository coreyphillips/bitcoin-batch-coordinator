import { X, Save } from 'lucide-react';
import { useState, useEffect } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { api } from '../lib/api';
import toast from 'react-hot-toast';

interface SettingsModalProps {
  onClose: () => void;
}

interface Config {
  network: string;
  min_participants: string;
  max_participants: string;
  timeout_seconds: string;
}

export function SettingsModal({ onClose }: SettingsModalProps) {
  const queryClient = useQueryClient();

  const { data: configData } = useQuery({
    queryKey: ['config'],
    queryFn: async () => {
      const response = await api.get('/config');
      return response.data as { config: Config };
    },
  });

  const [network, setNetwork] = useState('bitcoin');
  const [minParticipants, setMinParticipants] = useState('2');
  const [maxParticipants, setMaxParticipants] = useState('10');
  const [timeoutSeconds, setTimeoutSeconds] = useState('300');

  useEffect(() => {
    if (configData?.config) {
      setNetwork(configData.config.network);
      setMinParticipants(configData.config.min_participants);
      setMaxParticipants(configData.config.max_participants);
      setTimeoutSeconds(configData.config.timeout_seconds);
    }
  }, [configData]);

  const updateMutation = useMutation({
    mutationFn: async (config: Record<string, string>) => {
      const response = await api.post('/config', { config });
      return response.data;
    },
    onSuccess: () => {
      toast.success('Settings saved successfully!');
      queryClient.invalidateQueries({ queryKey: ['config'] });
      onClose();
    },
    onError: (error: any) => {
      toast.error(error?.response?.data?.message || 'Failed to save settings');
    },
  });

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();

    updateMutation.mutate({
      network,
      min_participants: minParticipants,
      max_participants: maxParticipants,
      timeout_seconds: timeoutSeconds,
    });
  };

  return (
    <div className="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50 p-4">
      <div className="glass rounded-2xl border border-white/10 max-w-2xl w-full max-h-[90vh] overflow-auto">
        {/* Header */}
        <div className="sticky top-0 glass border-b border-white/10 p-6 flex items-center justify-between">
          <h2 className="text-2xl font-bold text-white">Settings</h2>
          <button
            onClick={onClose}
            className="p-2 hover:bg-white/10 rounded-lg transition-colors"
          >
            <X className="w-6 h-6 text-gray-400" />
          </button>
        </div>

        <form onSubmit={handleSubmit} className="p-6 space-y-6">
          {/* Network Selection */}
          <div>
            <label className="block text-sm font-semibold text-white mb-2">
              Bitcoin Network
            </label>
            <select
              value={network}
              onChange={(e) => setNetwork(e.target.value)}
              className="w-full px-4 py-2 bg-white/5 border border-white/10 rounded-lg text-white focus:outline-none focus:ring-2 focus:ring-bitcoin-orange"
            >
              <option value="bitcoin">Bitcoin Mainnet</option>
              <option value="testnet">Testnet</option>
              <option value="signet">Signet</option>
              <option value="regtest">Regtest</option>
            </select>
            {network === 'bitcoin' && (
              <p className="text-xs text-yellow-400 mt-1 flex items-center gap-1">
                ⚠️ You are on mainnet - real Bitcoin will be used!
              </p>
            )}
          </div>

          {/* Batch Parameters */}
          <div className="glass rounded-xl p-4 border border-white/10">
            <h3 className="text-lg font-semibold text-white mb-4">Default Batch Parameters</h3>

            <div className="space-y-4">
              <div>
                <label className="block text-sm font-semibold text-white mb-2">
                  Minimum Participants
                </label>
                <input
                  type="number"
                  min="2"
                  value={minParticipants}
                  onChange={(e) => setMinParticipants(e.target.value)}
                  className="w-full px-4 py-2 bg-white/5 border border-white/10 rounded-lg text-white focus:outline-none focus:ring-2 focus:ring-bitcoin-orange"
                />
              </div>

              <div>
                <label className="block text-sm font-semibold text-white mb-2">
                  Maximum Participants
                </label>
                <input
                  type="number"
                  min={minParticipants}
                  value={maxParticipants}
                  onChange={(e) => setMaxParticipants(e.target.value)}
                  className="w-full px-4 py-2 bg-white/5 border border-white/10 rounded-lg text-white focus:outline-none focus:ring-2 focus:ring-bitcoin-orange"
                />
              </div>

              <div>
                <label className="block text-sm font-semibold text-white mb-2">
                  Timeout (seconds)
                </label>
                <input
                  type="number"
                  min="60"
                  step="60"
                  value={timeoutSeconds}
                  onChange={(e) => setTimeoutSeconds(e.target.value)}
                  className="w-full px-4 py-2 bg-white/5 border border-white/10 rounded-lg text-white focus:outline-none focus:ring-2 focus:ring-bitcoin-orange"
                />
                <p className="text-xs text-gray-400 mt-1">
                  {Math.floor(parseInt(timeoutSeconds) / 60)} minutes
                </p>
              </div>
            </div>
          </div>

          {/* Info Box */}
          <div className="glass rounded-xl p-4 border border-blue-500/30 bg-blue-500/10">
            <h3 className="text-sm font-semibold text-blue-400 mb-2">ℹ️ About Settings</h3>
            <div className="text-sm text-gray-300 space-y-1">
              <p>• Network changes require coordinator restart</p>
              <p>• Batch parameters apply to newly created batches</p>
              <p>• Existing batches are not affected by changes</p>
            </div>
          </div>

          {/* Actions */}
          <div className="flex gap-3">
            <button
              type="button"
              onClick={onClose}
              className="flex-1 px-4 py-2 bg-white/10 hover:bg-white/20 text-white rounded-lg transition-colors"
            >
              Cancel
            </button>
            <button
              type="submit"
              disabled={updateMutation.isPending}
              className="flex-1 px-4 py-2 gradient-orange text-white rounded-lg font-semibold hover:opacity-90 transition-opacity disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2"
            >
              <Save className="w-4 h-4" />
              {updateMutation.isPending ? 'Saving...' : 'Save Settings'}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
