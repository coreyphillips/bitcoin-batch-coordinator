import { X, Plus } from 'lucide-react';
import { useState } from 'react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { createBatch } from '../lib/api';
import toast from 'react-hot-toast';

interface CreateBatchModalProps {
  onClose: () => void;
}

export function CreateBatchModal({ onClose }: CreateBatchModalProps) {
  const queryClient = useQueryClient();
  const [minParticipants, setMinParticipants] = useState(2);
  const [maxParticipants, setMaxParticipants] = useState(10);
  const [timeoutSeconds, setTimeoutSeconds] = useState(300);

  const createMutation = useMutation({
    mutationFn: createBatch,
    onSuccess: (data) => {
      toast.success(`Batch created! ID: ${data.id.slice(0, 8)}...`);
      queryClient.invalidateQueries({ queryKey: ['batches'] });
      onClose();
    },
    onError: (error: any) => {
      toast.error(error?.response?.data?.message || 'Failed to create batch');
    },
  });

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();

    if (minParticipants < 2) {
      toast.error('Minimum participants must be at least 2');
      return;
    }

    if (maxParticipants < minParticipants) {
      toast.error('Maximum participants must be greater than minimum');
      return;
    }

    if (timeoutSeconds < 60) {
      toast.error('Timeout must be at least 60 seconds');
      return;
    }

    createMutation.mutate({
      min_participants: minParticipants,
      max_participants: maxParticipants,
      timeout_seconds: timeoutSeconds,
    });
  };

  return (
    <div className="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50 p-4">
      <div className="glass rounded-2xl border border-white/10 max-w-md w-full">
        {/* Header */}
        <div className="border-b border-white/10 p-6 flex items-center justify-between">
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 rounded-lg bg-gradient-orange flex items-center justify-center">
              <Plus className="w-6 h-6 text-white" />
            </div>
            <h2 className="text-2xl font-bold text-white">Create New Batch</h2>
          </div>
          <button
            onClick={onClose}
            className="p-2 hover:bg-white/10 rounded-lg transition-colors"
          >
            <X className="w-6 h-6 text-gray-400" />
          </button>
        </div>

        <form onSubmit={handleSubmit} className="p-6 space-y-6">
          {/* Min Participants */}
          <div>
            <label className="block text-sm font-semibold text-white mb-2">
              Minimum Participants
            </label>
            <input
              type="number"
              min="2"
              max="100"
              value={minParticipants}
              onChange={(e) => setMinParticipants(parseInt(e.target.value))}
              className="w-full px-4 py-2 bg-white/5 border border-white/10 rounded-lg text-white focus:outline-none focus:ring-2 focus:ring-bitcoin-orange"
              required
            />
            <p className="text-xs text-gray-400 mt-1">
              Minimum number of participants required to start batching
            </p>
          </div>

          {/* Max Participants */}
          <div>
            <label className="block text-sm font-semibold text-white mb-2">
              Maximum Participants
            </label>
            <input
              type="number"
              min={minParticipants}
              max="1000"
              value={maxParticipants}
              onChange={(e) => setMaxParticipants(parseInt(e.target.value))}
              className="w-full px-4 py-2 bg-white/5 border border-white/10 rounded-lg text-white focus:outline-none focus:ring-2 focus:ring-bitcoin-orange"
              required
            />
            <p className="text-xs text-gray-400 mt-1">
              Batch will close automatically when this limit is reached
            </p>
          </div>

          {/* Timeout */}
          <div>
            <label className="block text-sm font-semibold text-white mb-2">
              Timeout (seconds)
            </label>
            <input
              type="number"
              min="60"
              max="3600"
              step="60"
              value={timeoutSeconds}
              onChange={(e) => setTimeoutSeconds(parseInt(e.target.value))}
              className="w-full px-4 py-2 bg-white/5 border border-white/10 rounded-lg text-white focus:outline-none focus:ring-2 focus:ring-bitcoin-orange"
              required
            />
            <p className="text-xs text-gray-400 mt-1">
              Time to wait before closing batch ({Math.floor(timeoutSeconds / 60)} minutes)
            </p>
          </div>

          {/* Preview */}
          <div className="glass rounded-xl p-4 border border-white/10 bg-blue-500/10">
            <h3 className="text-sm font-semibold text-blue-400 mb-2">Batch Parameters</h3>
            <div className="space-y-1 text-sm text-white">
              <div className="flex justify-between">
                <span className="text-gray-400">Participants:</span>
                <span className="font-semibold">{minParticipants} - {maxParticipants}</span>
              </div>
              <div className="flex justify-between">
                <span className="text-gray-400">Timeout:</span>
                <span className="font-semibold">{Math.floor(timeoutSeconds / 60)} minutes</span>
              </div>
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
              disabled={createMutation.isPending}
              className="flex-1 px-4 py-2 gradient-orange text-white rounded-lg font-semibold hover:opacity-90 transition-opacity disabled:opacity-50 disabled:cursor-not-allowed"
            >
              {createMutation.isPending ? 'Creating...' : 'Create Batch'}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
