import { useQuery } from '@tanstack/react-query';
import { fetchBatches, fetchStats } from '../lib/api';
import { BatchCard } from '../components/BatchCard';
import { formatSatoshis } from '../lib/utils';
import { Activity, TrendingUp, Users, Zap } from 'lucide-react';

export function Dashboard() {
  const { data: batchesData, isLoading: batchesLoading } = useQuery({
    queryKey: ['batches'],
    queryFn: () => fetchBatches(),
    refetchInterval: 5000, // Refetch every 5 seconds
  });

  const { data: statsData, isLoading: statsLoading } = useQuery({
    queryKey: ['stats'],
    queryFn: () => fetchStats(7), // Last 7 days
    refetchInterval: 30000, // Refetch every 30 seconds
  });

  const activeBatches = batchesData?.batches.filter(
    (b) => b.state === 'filling' || b.state === 'ready' || b.state === 'signing'
  ) || [];

  return (
    <div className="min-h-screen bg-gradient-to-br from-bitcoin-dark via-gray-900 to-black">
      {/* Header */}
      <header className="border-b border-white/10 bg-black/30 backdrop-blur-xl">
        <div className="max-w-7xl mx-auto px-6 py-6">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-3">
              <div className="w-12 h-12 rounded-xl bg-gradient-orange flex items-center justify-center">
                <Zap className="w-7 h-7 text-white" />
              </div>
              <div>
                <h1 className="text-2xl font-bold text-white">Bitcoin Batch Coordinator</h1>
                <p className="text-sm text-gray-400">Save on transaction fees through batching</p>
              </div>
            </div>
          </div>
        </div>
      </header>

      {/* Main Content */}
      <main className="max-w-7xl mx-auto px-6 py-8">
        {/* Stats Grid */}
        <div className="grid grid-cols-1 md:grid-cols-3 gap-6 mb-8">
          <div className="glass rounded-2xl p-6 border border-white/10">
            <div className="flex items-center gap-3 mb-2">
              <div className="p-2 rounded-lg bg-green-500/20">
                <TrendingUp className="w-5 h-5 text-green-400" />
              </div>
              <span className="text-sm text-gray-400">Total Saved</span>
            </div>
            <p className="text-3xl font-bold text-white">
              {statsLoading ? '...' : formatSatoshis(statsData?.total_fees_saved || 0)}
            </p>
            <p className="text-xs text-gray-500 mt-1">Last 7 days</p>
          </div>

          <div className="glass rounded-2xl p-6 border border-white/10">
            <div className="flex items-center gap-3 mb-2">
              <div className="p-2 rounded-lg bg-blue-500/20">
                <Activity className="w-5 h-5 text-blue-400" />
              </div>
              <span className="text-sm text-gray-400">Batches Completed</span>
            </div>
            <p className="text-3xl font-bold text-white">
              {statsLoading ? '...' : statsData?.total_batches || 0}
            </p>
            <p className="text-xs text-gray-500 mt-1">Last 7 days</p>
          </div>

          <div className="glass rounded-2xl p-6 border border-white/10">
            <div className="flex items-center gap-3 mb-2">
              <div className="p-2 rounded-lg bg-purple-500/20">
                <Users className="w-5 h-5 text-purple-400" />
              </div>
              <span className="text-sm text-gray-400">Total Participants</span>
            </div>
            <p className="text-3xl font-bold text-white">
              {statsLoading ? '...' : statsData?.total_participants || 0}
            </p>
            <p className="text-xs text-gray-500 mt-1">Last 7 days</p>
          </div>
        </div>

        {/* Active Batches */}
        <div className="mb-8">
          <h2 className="text-2xl font-bold text-white mb-6">Active Batches</h2>
          {batchesLoading ? (
            <div className="text-center py-12 text-gray-400">Loading...</div>
          ) : activeBatches.length === 0 ? (
            <div className="glass rounded-2xl p-12 text-center border border-white/10">
              <Activity className="w-12 h-12 text-gray-600 mx-auto mb-4" />
              <p className="text-gray-400 text-lg">No active batches</p>
              <p className="text-gray-500 text-sm mt-2">
                Batches will appear here when they're being coordinated
              </p>
            </div>
          ) : (
            <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
              {activeBatches.map((batch) => (
                <BatchCard key={batch.id} batch={batch} />
              ))}
            </div>
          )}
        </div>

        {/* Recent Batches */}
        <div>
          <h2 className="text-2xl font-bold text-white mb-6">Recent Activity</h2>
          <div className="glass rounded-2xl border border-white/10 overflow-hidden">
            <div className="overflow-x-auto">
              <table className="w-full">
                <thead className="bg-white/5">
                  <tr>
                    <th className="px-6 py-4 text-left text-xs font-semibold text-gray-400 uppercase tracking-wider">
                      Batch ID
                    </th>
                    <th className="px-6 py-4 text-left text-xs font-semibold text-gray-400 uppercase tracking-wider">
                      State
                    </th>
                    <th className="px-6 py-4 text-left text-xs font-semibold text-gray-400 uppercase tracking-wider">
                      Participants
                    </th>
                    <th className="px-6 py-4 text-left text-xs font-semibold text-gray-400 uppercase tracking-wider">
                      Fees
                    </th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-white/5">
                  {batchesData?.batches.slice(0, 10).map((batch) => (
                    <tr key={batch.id} className="hover:bg-white/5 transition-colors">
                      <td className="px-6 py-4 text-sm font-mono text-gray-300">
                        {batch.id.slice(0, 8)}...
                      </td>
                      <td className="px-6 py-4 text-sm">
                        <span
                          className={`px-2 py-1 rounded-full text-xs font-semibold ${
                            batch.state === 'completed'
                              ? 'bg-green-500/20 text-green-400'
                              : batch.state === 'failed'
                              ? 'bg-red-500/20 text-red-400'
                              : 'bg-blue-500/20 text-blue-400'
                          }`}
                        >
                          {batch.state}
                        </span>
                      </td>
                      <td className="px-6 py-4 text-sm text-gray-300">
                        {batch.participant_count}
                      </td>
                      <td className="px-6 py-4 text-sm font-semibold text-bitcoin-orange">
                        {batch.total_fees ? formatSatoshis(batch.total_fees) : '-'}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>
        </div>
      </main>
    </div>
  );
}
