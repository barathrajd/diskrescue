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
  mountPoints: string[];
  eligibilityReason: string | null;
};

export async function listPhysicalSources() {
  return invoke<PhysicalSource[]>('list_physical_sources');
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
