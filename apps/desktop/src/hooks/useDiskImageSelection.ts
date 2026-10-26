import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { useState } from 'react';

export type SelectedImage = {
  path: string;
  name: string;
};

export type ImageReview = {
  imageBytes: number;
  mbrStatus: string;
  mbrPartitionCount: number;
  gptStatus: string;
  gptPartitionCount: number;
};

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
  kind: string | null;
  mountPoints: string[];
};

export type FilesystemFinding = {
  kind: string;
  offsetBytes: number;
  lengthBytes: number;
  confidence: string;
  reasons: string[];
};

export type PartitionFinding = {
  table: string;
  index: number;
  typeLabel: string;
  offsetBytes: number;
  lengthBytes: number;
  filesystem: FilesystemFinding | null;
  issue: string | null;
};

export type ImageScan = {
  imageBytes: number;
  mbrError: string | null;
  gptError: string | null;
  partitions: PartitionFinding[];
};

export type LostScanProgress = {
  scannedBytes: number;
  totalBytes: number;
};

export type LostImageScan = {
  imageBytes: number;
  scannedBytes: number;
  complete: boolean;
  candidates: PartitionFinding[];
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

export async function scanImage(path: string) {
  return invoke<ImageScan>('scan_image', { path });
}

export async function searchLostFilesystems(path: string) {
  return invoke<LostImageScan>('search_lost_filesystems', { path });
}

export async function scanNtfsDeletedFiles(path: string, partitionOffsetBytes: number, partitionLengthBytes: number) {
  return invoke<DeletedFileEntry[]>('scan_ntfs_deleted_files', {
    path,
    partitionOffsetBytes,
    partitionLengthBytes,
  });
}

export async function cancelLostFilesystemSearch() {
  await invoke('cancel_lost_filesystem_search');
}

export function useDiskImageSelection() {
  const [selectedImage, setSelectedImage] = useState<SelectedImage | null>(null);
  const [imageReview, setImageReview] = useState<ImageReview | null>(null);
  const [message, setMessage] = useState('');

  async function chooseImage() {
    setMessage('');
    try {
      const path = await open({
        multiple: false,
        directory: false,
        filters: [{ name: 'Disk images', extensions: ['img', 'dd', 'raw', 'dmg', 'iso'] }],
      });
      if (!path || typeof path !== 'string') return;

      setSelectedImage({ path, name: path.split(/[\\/]/).at(-1) ?? path });
      setImageReview(null);
      setImageReview(await invoke<ImageReview>('inspect_image', { path }));
    } catch (error) {
      setMessage(error instanceof Error ? error.message : 'Could not inspect this image.');
    }
  }

  return { selectedImage, imageReview, message, chooseImage };
}
