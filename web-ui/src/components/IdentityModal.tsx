import { X, Key, Upload, FileText, Copy, QrCode as QrCodeIcon } from 'lucide-react';
import { useState, useCallback } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { useDropzone } from 'react-dropzone';
import { QRCodeSVG } from 'qrcode.react';
import { api } from '../lib/api';
import { copyToClipboard } from '../lib/utils';
import toast from 'react-hot-toast';

interface IdentityModalProps {
  onClose: () => void;
  required?: boolean;
}

interface CurrentIdentity {
  pubkey: string;
  identity_type: string;
  created_at: number;
}

export function IdentityModal({ onClose, required = false }: IdentityModalProps) {
  const queryClient = useQueryClient();
  const [recoveryPhrase, setRecoveryPhrase] = useState('');
  const [selectedFile, setSelectedFile] = useState<File | null>(null);
  const [showQR, setShowQR] = useState(false);
  const [passphrase, setPassphrase] = useState('');

  // Fetch current identity
  const { data: currentIdentity } = useQuery<CurrentIdentity | null>({
    queryKey: ['identity'],
    queryFn: async () => {
      const response = await api.get('/identity/current');
      return response.data;
    },
  });

  const [activeTab, setActiveTab] = useState<'current' | 'import-file' | 'import-phrase'>(
    currentIdentity ? 'current' : 'import-file'
  );

  // Import from file mutation
  const importFileMutation = useMutation({
    mutationFn: async ({ file, pass }: { file: File; pass: string }) => {
      const formData = new FormData();
      formData.append('file', file);
      formData.append('passphrase', pass);
      const response = await api.post('/identity/import-file', formData, {
        headers: { 'Content-Type': 'multipart/form-data' },
      });
      return response.data;
    },
    onSuccess: (data) => {
      toast.success(`Identity imported! Pubkey: ${data.pubkey.slice(0, 16)}...`);
      queryClient.invalidateQueries({ queryKey: ['identity'] });
      if (currentIdentity) {
        setActiveTab('current');
      }
      setSelectedFile(null);
      setPassphrase('');
      if (required) {
        window.location.reload();
      }
    },
    onError: (error: any) => {
      toast.error(error?.response?.data?.message || 'Failed to import identity');
    },
  });

  // Import from phrase mutation
  const importPhraseMutation = useMutation({
    mutationFn: async ({ phrase, pass }: { phrase: string; pass: string }) => {
      const response = await api.post('/identity/import-phrase', {
        recovery_phrase: phrase,
        passphrase: pass,
      });
      return response.data;
    },
    onSuccess: (data) => {
      toast.success(`Identity imported! Pubkey: ${data.pubkey.slice(0, 16)}...`);
      queryClient.invalidateQueries({ queryKey: ['identity'] });
      if (currentIdentity) {
        setActiveTab('current');
      }
      setRecoveryPhrase('');
      setPassphrase('');
      if (required) {
        window.location.reload();
      }
    },
    onError: (error: any) => {
      toast.error(error?.response?.data?.message || 'Failed to import from phrase');
    },
  });

  // File drop handler
  const onDrop = useCallback((acceptedFiles: File[]) => {
    if (acceptedFiles.length > 0) {
      setSelectedFile(acceptedFiles[0]);
    }
  }, []);

  const { getRootProps, getInputProps, isDragActive } = useDropzone({
    onDrop,
    accept: {
      'application/octet-stream': ['.pkarr']
    },
    maxFiles: 1,
  });

  const handleImportFile = (e: React.FormEvent) => {
    e.preventDefault();
    if (!selectedFile || !passphrase) {
      toast.error('Please select a file and enter a passphrase');
      return;
    }
    importFileMutation.mutate({ file: selectedFile, pass: passphrase });
  };

  const handleImportPhrase = (e: React.FormEvent) => {
    e.preventDefault();
    const words = recoveryPhrase.trim().split(/\s+/);
    if (words.length !== 12 && words.length !== 24) {
      toast.error('Recovery phrase must be 12 or 24 words');
      return;
    }
    if (!passphrase) {
      toast.error('Please enter a passphrase');
      return;
    }
    importPhraseMutation.mutate({ phrase: recoveryPhrase, pass: passphrase });
  };

  const tabs = currentIdentity ? [
    { id: 'current' as const, label: 'Current Identity', icon: Key },
    { id: 'import-file' as const, label: 'Import File', icon: Upload },
    { id: 'import-phrase' as const, label: 'Import Phrase', icon: FileText },
  ] : [
    { id: 'import-file' as const, label: 'Import File', icon: Upload },
    { id: 'import-phrase' as const, label: 'Import Phrase', icon: FileText },
  ];

  return (
    <div className="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50 p-4">
      <div className="glass rounded-2xl border border-white/10 max-w-3xl w-full max-h-[90vh] overflow-auto">
        {/* Header */}
        <div className="sticky top-0 glass border-b border-white/10 p-6 flex items-center justify-between">
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 rounded-lg bg-gradient-orange flex items-center justify-center">
              <Key className="w-6 h-6 text-white" />
            </div>
            <div>
              <h2 className="text-2xl font-bold text-white">
                {required ? 'Setup Coordinator Identity' : 'Identity Management'}
              </h2>
              {required && (
                <p className="text-sm text-gray-400 mt-1">Import your identity to continue</p>
              )}
            </div>
          </div>
          {!required && (
            <button
              onClick={onClose}
              className="p-2 hover:bg-white/10 rounded-lg transition-colors"
            >
              <X className="w-6 h-6 text-gray-400" />
            </button>
          )}
        </div>

        {/* Tabs */}
        <div className="border-b border-white/10 px-6">
          <div className="flex gap-2 overflow-x-auto">
            {tabs.map((tab) => {
              const Icon = tab.icon;
              return (
                <button
                  key={tab.id}
                  onClick={() => setActiveTab(tab.id)}
                  className={`flex items-center gap-2 px-4 py-3 border-b-2 transition-colors whitespace-nowrap ${
                    activeTab === tab.id
                      ? 'border-bitcoin-orange text-white'
                      : 'border-transparent text-gray-400 hover:text-white'
                  }`}
                >
                  <Icon className="w-4 h-4" />
                  {tab.label}
                </button>
              );
            })}
          </div>
        </div>

        {/* Tab Content */}
        <div className="p-6">
          {/* Current Identity Tab */}
          {activeTab === 'current' && currentIdentity && (
            <div className="space-y-6">
              <div className="glass rounded-xl p-6 border border-white/10">
                <h3 className="text-lg font-semibold text-white mb-4">Active Coordinator Identity</h3>

                <div className="space-y-4">
                  <div>
                    <label className="text-sm text-gray-400">Public Key</label>
                    <div className="flex items-center gap-2 mt-1">
                      <code className="flex-1 px-3 py-2 bg-white/5 rounded-lg text-white font-mono text-sm break-all">
                        {currentIdentity.pubkey}
                      </code>
                      <button
                        onClick={() => {
                          copyToClipboard(currentIdentity.pubkey);
                          toast.success('Pubkey copied!');
                        }}
                        className="p-2 hover:bg-white/10 rounded-lg transition-colors"
                      >
                        <Copy className="w-4 h-4 text-gray-400" />
                      </button>
                      <button
                        onClick={() => setShowQR(!showQR)}
                        className="p-2 hover:bg-white/10 rounded-lg transition-colors"
                      >
                        <QrCodeIcon className="w-4 h-4 text-gray-400" />
                      </button>
                    </div>
                  </div>

                  {showQR && (
                    <div className="flex justify-center p-4 bg-white rounded-lg">
                      <QRCodeSVG value={currentIdentity.pubkey} size={200} />
                    </div>
                  )}

                  <div className="grid grid-cols-2 gap-4">
                    <div>
                      <label className="text-sm text-gray-400">Type</label>
                      <div className="text-white font-semibold mt-1 capitalize">
                        {currentIdentity.identity_type}
                      </div>
                    </div>
                    <div>
                      <label className="text-sm text-gray-400">Created</label>
                      <div className="text-white font-semibold mt-1">
                        {new Date(currentIdentity.created_at * 1000).toLocaleDateString()}
                      </div>
                    </div>
                  </div>
                </div>
              </div>

              <div className="glass rounded-xl p-4 border border-yellow-500/30 bg-yellow-500/10">
                <p className="text-sm text-yellow-400">
                  ⚠️ This is your coordinator's public identity. Share this pubkey with participants who want to join your batches.
                </p>
              </div>
            </div>
          )}

          {/* Import File Tab */}
          {activeTab === 'import-file' && (
            <form onSubmit={handleImportFile} className="space-y-6">
              <div>
                <label className="block text-sm font-semibold text-white mb-2">
                  Upload .pkarr File
                </label>
                <div
                  {...getRootProps()}
                  className={`border-2 border-dashed rounded-xl p-8 text-center transition-colors cursor-pointer ${
                    isDragActive
                      ? 'border-bitcoin-orange bg-bitcoin-orange/10'
                      : 'border-white/20 hover:border-white/40'
                  }`}
                >
                  <input {...getInputProps()} />
                  <Upload className="w-12 h-12 text-gray-400 mx-auto mb-3" />
                  {selectedFile ? (
                    <div className="text-white">
                      <p className="font-semibold">{selectedFile.name}</p>
                      <p className="text-sm text-gray-400 mt-1">
                        {(selectedFile.size / 1024).toFixed(2)} KB
                      </p>
                    </div>
                  ) : (
                    <div className="text-gray-400">
                      <p>Drag & drop your .pkarr file here</p>
                      <p className="text-sm mt-1">or click to browse</p>
                    </div>
                  )}
                </div>
              </div>

              <div>
                <label className="block text-sm font-semibold text-white mb-2">
                  Passphrase
                </label>
                <input
                  type="password"
                  value={passphrase}
                  onChange={(e) => setPassphrase(e.target.value)}
                  className="w-full px-4 py-2 bg-white/5 border border-white/10 rounded-lg text-white focus:outline-none focus:ring-2 focus:ring-bitcoin-orange"
                  placeholder="Enter your passphrase"
                  required
                />
              </div>

              <button
                type="submit"
                disabled={!selectedFile || !passphrase || importFileMutation.isPending}
                className="w-full px-4 py-2 gradient-orange text-white rounded-lg font-semibold hover:opacity-90 transition-opacity disabled:opacity-50 disabled:cursor-not-allowed"
              >
                {importFileMutation.isPending ? 'Importing...' : 'Import Identity'}
              </button>
            </form>
          )}

          {/* Import Phrase Tab */}
          {activeTab === 'import-phrase' && (
            <form onSubmit={handleImportPhrase} className="space-y-6">
              <div>
                <label className="block text-sm font-semibold text-white mb-2">
                  Recovery Phrase (12 or 24 words)
                </label>
                <textarea
                  value={recoveryPhrase}
                  onChange={(e) => setRecoveryPhrase(e.target.value)}
                  rows={4}
                  className="w-full px-4 py-2 bg-white/5 border border-white/10 rounded-lg text-white focus:outline-none focus:ring-2 focus:ring-bitcoin-orange font-mono text-sm"
                  placeholder="word1 word2 word3 ..."
                  required
                />
                <p className="text-xs text-gray-400 mt-1">
                  Words: {recoveryPhrase.trim().split(/\s+/).filter(w => w).length}
                </p>
              </div>

              <div>
                <label className="block text-sm font-semibold text-white mb-2">
                  Passphrase
                </label>
                <input
                  type="password"
                  value={passphrase}
                  onChange={(e) => setPassphrase(e.target.value)}
                  className="w-full px-4 py-2 bg-white/5 border border-white/10 rounded-lg text-white focus:outline-none focus:ring-2 focus:ring-bitcoin-orange"
                  placeholder="Enter a passphrase to encrypt your identity"
                  required
                />
              </div>

              <button
                type="submit"
                disabled={!recoveryPhrase || !passphrase || importPhraseMutation.isPending}
                className="w-full px-4 py-2 gradient-orange text-white rounded-lg font-semibold hover:opacity-90 transition-opacity disabled:opacity-50 disabled:cursor-not-allowed"
              >
                {importPhraseMutation.isPending ? 'Importing...' : 'Import from Phrase'}
              </button>
            </form>
          )}
        </div>
      </div>
    </div>
  );
}
