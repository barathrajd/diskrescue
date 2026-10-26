import { invoke } from '@tauri-apps/api/core';

export type PhysicalSource = {
  id: string;
  model: string | null;
  serial: string | null;
  capacityBytes: number;
  mountStateKnown: boolean;
  systemStateKnown: boolean;
  isSystem: boolean;
  externalStateKnown: boolean;
  isExternal: boolean;
  mountPoints: string[];
  partitions: PhysicalPartition[];
  eligibilityReason: string | null;
};

export type PhysicalPartition = {
  id: string;
  name: string | null;
  sizeBytes: number;
  offsetBytes: number | null;
  kind: string | null;
  mountPoints: string[];
};

export type DeletedFileEntry = {
  recordNumber: number;
  sequenceNumber: number;
  isDirectory: boolean;
  name: string | null;
  parentRecordNumber: number | null;
  dataSizeBytes: number | null;
};

export async function listPhysicalSources() {
  return invoke<PhysicalSource[]>('list_physical_sources');
}

export async function scanSelectedNtfsPartition(source: PhysicalSource, partition: PhysicalPartition) {
  if (partition.offsetBytes === null || partition.sizeBytes <= 0) {
    throw new Error('Partition bounds are unavailable; refusing to scan an unverified range.');
  }
  return invoke<DeletedFileEntry[]>('scan_selected_ntfs_partition', {
    sourceId: source.id,
    expectedModel: source.model,
    expectedSerial: source.serial,
    expectedCapacityBytes: source.capacityBytes,
    partitionId: partition.id,
    expectedPartitionOffsetBytes: partition.offsetBytes,
    expectedPartitionSizeBytes: partition.sizeBytes,
  });
}

export async function cancelLostFilesystemSearch() {
  await invoke('cancel_lost_filesystem_search');
}
