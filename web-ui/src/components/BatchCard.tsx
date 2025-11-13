import type { Batch } from '../lib/api';
import { formatRelativeTime, formatSatoshis, cn } from '../lib/utils';
import { Users, Clock, CheckCircle, XCircle } from 'lucide-react';

interface BatchCardProps {
  batch: Batch;
  onClick?: () => void;
}

export function BatchCard({ batch, onClick }: BatchCardProps) {
  const stateColors = {
    filling: 'border-blue-500/30 bg-blue-500/10',
    ready: 'border-yellow-500/30 bg-yellow-500/10',
    signing: 'border-purple-500/30 bg-purple-500/10',
    completed: 'border-green-500/30 bg-green-500/10',
    failed: 'border-red-500/30 bg-red-500/10',
  };

  const stateIcons = {
    filling: <Clock className="w-5 h-5 text-blue-400" />,
    ready: <Users className="w-5 h-5 text-yellow-400" />,
    signing: <Clock className="w-5 h-5 text-purple-400 animate-pulse" />,
    completed: <CheckCircle className="w-5 h-5 text-green-400" />,
    failed: <XCircle className="w-5 h-5 text-red-400" />,
  };

  return (
    <div
      onClick={onClick}
      className={cn(
        'glass rounded-xl p-6 border-2 transition-all cursor-pointer hover:scale-105',
        stateColors[batch.state]
      )}
    >
      <div className="flex items-start justify-between mb-4">
        <div className="flex items-center gap-2">
          {stateIcons[batch.state]}
          <span className="text-sm font-semibold uppercase tracking-wide">
            {batch.state}
          </span>
        </div>
        <span className="text-xs text-gray-400">
          {formatRelativeTime(batch.created_at)}
        </span>
      </div>

      <div className="space-y-3">
        <div className="flex items-center justify-between">
          <span className="text-sm text-gray-400">Participants</span>
          <span className="text-lg font-bold">{batch.participant_count}</span>
        </div>

        {batch.total_fees && (
          <div className="flex items-center justify-between">
            <span className="text-sm text-gray-400">Total Fees</span>
            <span className="text-lg font-bold text-bitcoin-orange">
              {formatSatoshis(batch.total_fees)}
            </span>
          </div>
        )}

        {batch.txid && (
          <div className="mt-4 pt-4 border-t border-white/10">
            <span className="text-xs font-mono text-gray-400 break-all">
              {batch.txid.slice(0, 16)}...
            </span>
          </div>
        )}
      </div>
    </div>
  );
}
