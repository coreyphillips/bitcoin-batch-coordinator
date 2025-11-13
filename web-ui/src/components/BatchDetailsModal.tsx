import { X, Users, Clock, CheckCircle, Hash, Copy } from 'lucide-react';
import type { Batch, BatchParticipant } from '../lib/api';
import { formatTimestamp, formatSatoshis, copyToClipboard, truncateHash } from '../lib/utils';
import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api } from '../lib/api';
import toast from 'react-hot-toast';

interface BatchDetailsModalProps {
  batch: Batch;
  onClose: () => void;
}

export function BatchDetailsModal({ batch, onClose }: BatchDetailsModalProps) {
  const [copied, setCopied] = useState<string | null>(null);

  const { data: participantsData } = useQuery({
    queryKey: ['batch-participants', batch.id],
    queryFn: async () => {
      const response = await api.get(`/batches/${batch.id}/participants`);
      return response.data as { participants: BatchParticipant[]; total: number };
    },
  });

  const handleCopy = (text: string, label: string) => {
    copyToClipboard(text);
    setCopied(label);
    toast.success(`${label} copied to clipboard!`);
    setTimeout(() => setCopied(null), 2000);
  };

  const stateColors: Record<string, string> = {
    filling: 'bg-blue-500/20 text-blue-400 border-blue-500/30',
    ready: 'bg-yellow-500/20 text-yellow-400 border-yellow-500/30',
    signing: 'bg-purple-500/20 text-purple-400 border-purple-500/30',
    completed: 'bg-green-500/20 text-green-400 border-green-500/30',
    failed: 'bg-red-500/20 text-red-400 border-red-500/30',
  };

  return (
    <div className="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50 p-4">
      <div className="glass rounded-2xl border border-white/10 max-w-3xl w-full max-h-[90vh] overflow-auto">
        {/* Header */}
        <div className="sticky top-0 glass border-b border-white/10 p-6 flex items-center justify-between">
          <div>
            <h2 className="text-2xl font-bold text-white">Batch Details</h2>
            <p className="text-sm text-gray-400 mt-1">ID: {truncateHash(batch.id, 8, 8)}</p>
          </div>
          <button
            onClick={onClose}
            className="p-2 hover:bg-white/10 rounded-lg transition-colors"
          >
            <X className="w-6 h-6 text-gray-400" />
          </button>
        </div>

        <div className="p-6 space-y-6">
          {/* Status Badge */}
          <div className="flex items-center gap-4">
            <span
              className={`px-4 py-2 rounded-full text-sm font-semibold uppercase tracking-wide border ${
                stateColors[batch.state]
              }`}
            >
              {batch.state}
            </span>
            {batch.state === 'signing' && (
              <div className="flex items-center gap-2 text-purple-400 animate-pulse">
                <Clock className="w-4 h-4" />
                <span className="text-sm">Waiting for signatures...</span>
              </div>
            )}
            {batch.state === 'completed' && (
              <div className="flex items-center gap-2 text-green-400">
                <CheckCircle className="w-4 h-4" />
                <span className="text-sm">Broadcast complete</span>
              </div>
            )}
          </div>

          {/* Info Grid */}
          <div className="grid grid-cols-2 gap-4">
            <div className="glass rounded-xl p-4 border border-white/10">
              <div className="text-sm text-gray-400 mb-1">Created</div>
              <div className="text-lg font-semibold text-white">
                {formatTimestamp(batch.created_at)}
              </div>
            </div>

            <div className="glass rounded-xl p-4 border border-white/10">
              <div className="text-sm text-gray-400 mb-1">Participants</div>
              <div className="text-lg font-semibold text-white flex items-center gap-2">
                <Users className="w-5 h-5 text-blue-400" />
                {batch.participant_count}
              </div>
            </div>

            {batch.completed_at && (
              <div className="glass rounded-xl p-4 border border-white/10">
                <div className="text-sm text-gray-400 mb-1">Completed</div>
                <div className="text-lg font-semibold text-white">
                  {formatTimestamp(batch.completed_at)}
                </div>
              </div>
            )}

            {batch.total_fees && (
              <div className="glass rounded-xl p-4 border border-white/10">
                <div className="text-sm text-gray-400 mb-1">Total Fees</div>
                <div className="text-lg font-semibold text-bitcoin-orange">
                  {formatSatoshis(batch.total_fees)}
                </div>
              </div>
            )}
          </div>

          {/* Transaction ID */}
          {batch.txid && (
            <div className="glass rounded-xl p-4 border border-white/10">
              <div className="flex items-center justify-between mb-2">
                <div className="flex items-center gap-2 text-sm text-gray-400">
                  <Hash className="w-4 h-4" />
                  Transaction ID
                </div>
                <button
                  onClick={() => handleCopy(batch.txid!, 'Transaction ID')}
                  className="p-1 hover:bg-white/10 rounded transition-colors"
                >
                  <Copy className={`w-4 h-4 ${copied === 'Transaction ID' ? 'text-green-400' : 'text-gray-400'}`} />
                </button>
              </div>
              <div className="font-mono text-sm text-white break-all">{batch.txid}</div>
              <a
                href={`https://mempool.space/tx/${batch.txid}`}
                target="_blank"
                rel="noopener noreferrer"
                className="text-sm text-bitcoin-orange hover:underline mt-2 inline-block"
              >
                View on Mempool.space →
              </a>
            </div>
          )}

          {/* Participants List */}
          {participantsData && participantsData.participants.length > 0 && (
            <div className="glass rounded-xl p-4 border border-white/10">
              <h3 className="text-lg font-semibold text-white mb-4 flex items-center gap-2">
                <Users className="w-5 h-5 text-blue-400" />
                Participants ({participantsData.total})
              </h3>
              <div className="space-y-2">
                {participantsData.participants.map((participant) => (
                  <div
                    key={participant.id}
                    className="flex items-center justify-between p-3 bg-white/5 rounded-lg"
                  >
                    <div className="flex items-center gap-3">
                      <div className="w-8 h-8 rounded-full bg-gradient-to-br from-bitcoin-orange to-orange-600 flex items-center justify-center text-white font-bold text-sm">
                        {participant.pubkey.slice(0, 2).toUpperCase()}
                      </div>
                      <div>
                        <div className="font-mono text-sm text-white">
                          {truncateHash(participant.pubkey, 8, 8)}
                        </div>
                        <div className="text-xs text-gray-400">
                          Joined {formatTimestamp(participant.joined_at)}
                        </div>
                      </div>
                    </div>
                    <div className="flex items-center gap-2">
                      {participant.signed && (
                        <span className="px-2 py-1 bg-green-500/20 text-green-400 text-xs rounded-full">
                          Signed
                        </span>
                      )}
                      <button
                        onClick={() => handleCopy(participant.pubkey, 'Public Key')}
                        className="p-1 hover:bg-white/10 rounded transition-colors"
                      >
                        <Copy className="w-4 h-4 text-gray-400" />
                      </button>
                    </div>
                  </div>
                ))}
              </div>
            </div>
          )}

          {/* Intent Data (if available) */}
          {batch.intent_data && (
            <details className="glass rounded-xl border border-white/10">
              <summary className="p-4 cursor-pointer text-white font-semibold hover:bg-white/5 transition-colors">
                Raw Intent Data
              </summary>
              <div className="p-4 pt-0">
                <pre className="text-xs text-gray-400 overflow-auto">
                  {JSON.stringify(JSON.parse(batch.intent_data), null, 2)}
                </pre>
              </div>
            </details>
          )}
        </div>

        {/* Footer Actions */}
        <div className="sticky bottom-0 glass border-t border-white/10 p-4 flex justify-end gap-3">
          <button
            onClick={onClose}
            className="px-4 py-2 bg-white/10 hover:bg-white/20 text-white rounded-lg transition-colors"
          >
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
