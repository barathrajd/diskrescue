import { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { cancelLostFilesystemSearch, listPhysicalSources, scanImage, scanNtfsDeletedFiles, searchLostFilesystems, useDiskImageSelection, type DeletedFileEntry, type ImageScan, type LostImageScan, type LostScanProgress, type PhysicalSource, type PartitionFinding } from './hooks/useDiskImageSelection';

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 bytes';
  const units = ['bytes', 'KB', 'MB', 'GB', 'TB'];
  const unitIndex = Math.min(
    Math.floor(Math.log(bytes) / Math.log(1024)),
    units.length - 1,
  );
  const value = bytes / 1024 ** unitIndex;
  return `${value.toLocaleString(undefined, {
    maximumFractionDigits: unitIndex === 0 ? 0 : 1,
  })} ${units[unitIndex]}`;
}

function App() {
  const { selectedImage, imageReview, message, chooseImage } = useDiskImageSelection();
  const [physicalSources, setPhysicalSources] = useState<PhysicalSource[] | null>(null);
  const [selectedSourceId, setSelectedSourceId] = useState<string | null>(null);
  const [selectedPartitionId, setSelectedPartitionId] = useState<string | null>(null);
  const [sourceMessage, setSourceMessage] = useState('');
  const [sourcesLoading, setSourcesLoading] = useState(false);
  const [imageScan, setImageScan] = useState<{ path: string; result: ImageScan } | null>(null);
  const [scanMessage, setScanMessage] = useState('');
  const [scanLoading, setScanLoading] = useState(false);
  const [lostImageScan, setLostImageScan] = useState<{ path: string; result: LostImageScan } | null>(null);
  const [lostScanProgress, setLostScanProgress] = useState<LostScanProgress | null>(null);
  const [lostScanMessage, setLostScanMessage] = useState('');
  const [lostScanLoading, setLostScanLoading] = useState(false);
  const [deletedFileScan, setDeletedFileScan] = useState<{ path: string; partitionOffsetBytes: number; entries: DeletedFileEntry[] } | null>(null);
  const [deletedFileScanMessage, setDeletedFileScanMessage] = useState('');
  const [deletedFileScanLoading, setDeletedFileScanLoading] = useState(false);
  const [fileCategory, setFileCategory] = useState<'photos' | 'videos' | 'all'>('all');
  const selectedSource = physicalSources?.find((source) => source.id === selectedSourceId) ?? null;

  async function refreshPhysicalSources() {
    setSourceMessage('');
    setSourcesLoading(true);
    try {
      const sources = await listPhysicalSources();
      setPhysicalSources(sources);
      setSelectedSourceId((current) =>
        current && sources.some((source) => source.id === current)
          ? current
          : sources[0]?.id ?? null,
      );
    } catch (error) {
      setSourceMessage(error instanceof Error ? error.message : 'Could not list physical disks.');
    } finally {
      setSourcesLoading(false);
    }
  }

  useEffect(() => {
    void refreshPhysicalSources();
  }, []);

  async function scanSelectedImage() {
    if (!selectedImage) return;
    setScanMessage('');
    setScanLoading(true);
    try {
      setImageScan({ path: selectedImage.path, result: await scanImage(selectedImage.path) });
    } catch (error) {
      setImageScan(null);
      setScanMessage(error instanceof Error ? error.message : 'Could not scan this image.');
    } finally {
      setScanLoading(false);
    }
  }

  async function findLostVolumes() {
    if (!selectedImage) return;
    setLostScanMessage('');
    setLostImageScan(null);
    setLostScanProgress(null);
    setLostScanLoading(true);
    let unlisten: (() => void) | undefined;
    try {
      unlisten = await listen<LostScanProgress>('lost-filesystem-progress', (event) => {
        setLostScanProgress(event.payload);
      });
      const result = await searchLostFilesystems(selectedImage.path);
      setLostImageScan({ path: selectedImage.path, result });
    } catch (error) {
      setLostScanMessage(error instanceof Error ? error.message : 'Could not search this image.');
    } finally {
      unlisten?.();
      setLostScanLoading(false);
    }
  }

  async function cancelLostSearch() {
    try {
      await cancelLostFilesystemSearch();
    } catch (error) {
      setLostScanMessage(error instanceof Error ? error.message : 'Could not cancel the search.');
    }
  }

  async function scanDeletedEntries(partition: PartitionFinding) {
    if (!selectedImage || partition.filesystem?.kind !== 'NTFS') return;
    setDeletedFileScanMessage('');
    setDeletedFileScanLoading(true);
    try {
      const entries = await scanNtfsDeletedFiles(selectedImage.path, partition.offsetBytes, partition.lengthBytes);
      setDeletedFileScan({ path: selectedImage.path, partitionOffsetBytes: partition.offsetBytes, entries });
    } catch (error) {
      setDeletedFileScan(null);
      setDeletedFileScanMessage(error instanceof Error ? error.message : 'Could not read deleted NTFS entries.');
    } finally {
      setDeletedFileScanLoading(false);
    }
  }

  const visibleImageScan =
    imageScan && selectedImage && imageScan.path === selectedImage.path
      ? imageScan.result
      : null;
  const visibleLostImageScan =
    lostImageScan && selectedImage && lostImageScan.path === selectedImage.path
      ? lostImageScan.result
      : null;
  const visibleDeletedFileScan = deletedFileScan && selectedImage && deletedFileScan.path === selectedImage.path
    ? deletedFileScan
    : null;
  const filteredDeletedEntries = visibleDeletedFileScan?.entries.filter((entry) => {
    if (!entry.name || entry.isDirectory) return false;
    const extension = entry.name.split('.').at(-1)?.toLowerCase() ?? '';
    const photoExtensions = ['jpg', 'jpeg', 'png', 'gif', 'bmp', 'tif', 'tiff', 'heic', 'webp'];
    const videoExtensions = ['mp4', 'mov', 'avi', 'mkv', 'mts', 'm4v', '3gp', 'wmv'];
    if (fileCategory === 'photos') return photoExtensions.includes(extension);
    if (fileCategory === 'videos') return videoExtensions.includes(extension);
    return true;
  }) ?? [];
  const lostScanPercent = lostScanProgress && lostScanProgress.totalBytes > 0
    ? Math.min(100, Math.floor(lostScanProgress.scannedBytes / lostScanProgress.totalBytes * 100))
    : 0;

  return (
    <main className="app-shell">
      <aside className="rail" aria-label="Application">
        <a className="brand" href="#top" aria-label="DiskRescue home">
          <span className="brand-mark" aria-hidden="true">
            <span />
            <span />
            <span />
          </span>
          <span className="brand-name">disk<span>rescue</span></span>
        </a>

        <div className="rail-section-label">WORKSPACE</div>
        <div className="nav-item nav-item-active" aria-current="page">
          <span className="nav-icon" aria-hidden="true">◫</span>
          <span>Image recovery</span>
          <span className="nav-dot" />
        </div>

        <div className="rail-bottom">
          <div className="offline-mark"><span /> OFFLINE MODE</div>
          <div className="rail-version">BUILD 0.1.0 · PREVIEW</div>
        </div>
      </aside>

      <section className="workspace" id="top">
        <header className="topbar">
          <div className="breadcrumb"><span>WORKSPACE</span><b>/</b> IMAGE RECOVERY</div>
          <div className="topbar-status"><span className="status-light" /> LOCAL SESSION</div>
        </header>

        <div className="content">
          <div className="hero-grid">
            <div className="hero-copy">
              <div className="eyebrow"><span className="eyebrow-line" /> A CAREFUL WAY BACK</div>
              <h1>Choose the external drive first.<br /><em>Keep it untouched.</em></h1>
              <p className="hero-description">
                Select an external source, then work from a disk image. Inspect NTFS metadata for
                deleted names while keeping the source drive untouched.
              </p>
            </div>
            <div className="hero-index" aria-hidden="true">
              <span className="hero-index-number">01</span>
              <span className="hero-index-rule" />
              <span className="hero-index-caption">READ-ONLY<br />BY DESIGN</span>
            </div>
          </div>

          <section className="safety-strip" aria-label="Safety requirements">
            <div className="safety-icon" aria-hidden="true">!</div>
            <div className="safety-copy">
              <strong>Do not mount, repair, or scan the affected drive.</strong>
              <span>DiskRescue keeps internal drives locked. Use an existing disk image below; this preview cannot image a physical drive yet.</span>
            </div>
            <span className="safety-tag">SOURCE PROTECTED</span>
          </section>

          <section className="source-inventory" aria-labelledby="physical-sources-heading">
            <div className="inventory-heading">
              <div>
                <div className="panel-kicker">SOURCE / 01 · EXTERNAL ONLY</div>
                <h2 id="physical-sources-heading">Select an external drive</h2>
                <p>Internal and unclassified drives are hidden. Listing reads device metadata only.</p>
              </div>
              <button className="browse-button inventory-refresh" onClick={refreshPhysicalSources} disabled={sourcesLoading} type="button">
                {sourcesLoading ? 'Checking…' : 'Refresh drives'} <span aria-hidden="true">↻</span>
              </button>
            </div>
            {sourceMessage && <p className="input-message" role="alert">{sourceMessage}</p>}
            {physicalSources && (physicalSources.length === 0 ? (
              <p className="inventory-empty" aria-live="polite">No confirmed external drives were reported. Drives with unknown type stay hidden.</p>
            ) : (
              <div className="source-list" aria-live="polite">
                {physicalSources.map((source) => {
                  const isSelected = source.id === selectedSourceId;
                  return (
                    <article className={`source-row${isSelected ? ' source-row-selected' : ''}`} key={source.id}>
                      <div className="source-device-icon" aria-hidden="true">▤</div>
                      <div className="source-main">
                        <strong>{source.model ?? 'Model unavailable'}</strong>
                        <span>{source.id} · {formatBytes(source.capacityBytes)}</span>
                        <small>Serial: {source.serial ?? 'Unavailable'} · {source.isSystem ? 'System disk' : source.systemStateKnown ? 'Not system disk' : 'System state unknown'} · {source.mountPoints.length ? `Mounted at ${source.mountPoints.join(', ')}` : source.mountStateKnown ? 'No mounted volumes found' : 'Mount state unknown'}</small>
                      </div>
                      <div className={source.eligibilityReason ? 'source-eligibility blocked' : 'source-eligibility'}>
                        <span>{source.eligibilityReason ?? 'External drive detected'}</span>
                        <small>Metadata only · imaging unavailable in this preview</small>
                      </div>
                      <button
                        className="source-select-button"
                        type="button"
                        aria-pressed={isSelected}
                          onClick={() => {
                            setSelectedSourceId(source.id);
                            setSelectedPartitionId(null);
                          }}
                      >
                        {isSelected ? 'Selected' : 'Select drive'}
                      </button>
                    </article>
                  );
                })}
              </div>
            ))}
            {selectedSourceId && (
              <p className="source-safety-note">Source selected for this session. Physical imaging is unavailable in this preview; no disk contents have been read.</p>
            )}
            <p className="source-safety-note">Do not mount or repair the original. Partition details below come from device metadata only.</p>
          </section>

          <div className="work-grid">
            <section className="image-panel" aria-labelledby="image-heading">
              <div className="panel-heading">
                <div>
                  <div className="panel-kicker">PARTITIONS / 02 · DEVICE METADATA</div>
                  <h2 id="image-heading">Partitions on the selected drive</h2>
                </div>
                <button className="browse-button inventory-refresh" onClick={chooseImage} type="button">
                  {selectedImage ? 'Choose another image' : 'Use existing image'} <span aria-hidden="true">↗</span>
                </button>
              </div>

              {!selectedSource ? (
                <p className="inventory-empty">Select an external drive above to load its partition list.</p>
              ) : selectedSource.partitions.length === 0 ? (
                <p className="inventory-empty">No partitions were reported for {selectedSource.model ?? selectedSource.id}.</p>
              ) : (
                <div className="partition-list" aria-label="External drive partitions">
                  {selectedSource.partitions.map((partition) => {
                    const active = partition.id === selectedPartitionId;
                    return (
                      <button
                        className={`partition-row${active ? ' partition-row-selected' : ''}`}
                        key={partition.id}
                        type="button"
                        aria-pressed={active}
                        onClick={() => setSelectedPartitionId(partition.id)}
                      >
                        <span className="partition-icon" aria-hidden="true">▤</span>
                        <span className="partition-main"><strong>{partition.name ?? partition.id}</strong><small>{partition.id} · {partition.kind ?? 'Filesystem unknown'}</small></span>
                        <span className="partition-size">{formatBytes(partition.sizeBytes)}</span>
                        <span className="partition-mount">{partition.mountPoints.length ? `Mounted: ${partition.mountPoints.join(', ')}` : 'Not mounted'}</span>
                      </button>
                    );
                  })}
                </div>
              )}

              {selectedPartitionId && (
                <p className="partition-safety-note">Partition selected. This build lists partition metadata here; recovery scans still require an existing image. Selecting a partition does not read or mount it.</p>
              )}
              {selectedImage && (
                <div className="selected-file selected-image-inline" aria-live="polite">
                  <div className="file-emblem" aria-hidden="true">IMG</div>
                  <div className="file-details">
                    <strong title={selectedImage.name}>{selectedImage.name}</strong>
                    <span>{imageReview ? formatBytes(imageReview.imageBytes) : 'Selected locally'} <i /> Read-only image</span>
                  </div>
                </div>
              )}

              {message && <p className="input-message" role="alert">{message}</p>}

              {imageReview && (
                <div className="image-review" aria-live="polite">
                  <div><span>MBR</span><strong>{imageReview.mbrStatus}</strong><small>{imageReview.mbrPartitionCount} partitions</small></div>
                  <div><span>GPT</span><strong>{imageReview.gptStatus}</strong><small>{imageReview.gptPartitionCount} partitions</small></div>
                </div>
              )}

              {imageReview && (
                <div className="scan-actions">
                  <p>Check partition entries for supported NTFS and ext4 structures.</p>
                  <button className="browse-button inventory-refresh" onClick={scanSelectedImage} disabled={scanLoading} type="button">
                    {scanLoading ? 'Scanning…' : 'Scan partitions'} <span aria-hidden="true">⌕</span>
                  </button>
                </div>
              )}
              {scanMessage && <p className="input-message" role="alert">{scanMessage}</p>}
              {visibleImageScan && (
                <div className="scan-results" aria-live="polite">
                  <div className="scan-results-heading">
                    <strong>Partition scan</strong>
                    <span>{visibleImageScan.partitions.length} entries · read-only</span>
                  </div>
                  {visibleImageScan.partitions.length === 0 ? (
                    <p className="scan-empty">No valid partition entries were found in the primary tables.</p>
                  ) : visibleImageScan.partitions.map((partition) => (
                    <article className="scan-result-row" key={`${partition.table}-${partition.index}`}>
                      <div className="scan-result-main">
                        <strong>{partition.filesystem?.kind ?? 'Unsupported or unknown filesystem'}</strong>
                        <span>{partition.table} entry {partition.index} · {partition.typeLabel}</span>
                        <small>Start {formatBytes(partition.offsetBytes)} · Length {formatBytes(partition.lengthBytes)}</small>
                        {partition.filesystem?.reasons.map((reason) => <small key={reason}>✓ {reason}</small>)}
                        {partition.issue && <small className="scan-issue">{partition.issue}</small>}
                      </div>
                      <span className={partition.filesystem ? 'scan-confidence' : 'scan-confidence unknown'}>
                        {partition.filesystem ? `${partition.filesystem.confidence} confidence` : 'Needs review'}
                      </span>
                      {partition.filesystem?.kind === 'NTFS' && (
                        <button
                          className="source-select-button"
                          type="button"
                          disabled={deletedFileScanLoading}
                          onClick={() => void scanDeletedEntries(partition)}
                        >
                          {deletedFileScanLoading ? 'Reading…' : 'List deleted files'}
                        </button>
                      )}
                    </article>
                  ))}
                  {(visibleImageScan.mbrError || visibleImageScan.gptError) && (
                    <p className="scan-warning">Primary table notes: {[visibleImageScan.mbrError, visibleImageScan.gptError].filter(Boolean).join(' · ')}</p>
                  )}
                  <p className="scan-limitation">This scan checks primary partition-table entries only. Lost partition tables and backup GPT scanning are not available yet.</p>
                </div>
              )}

              {imageReview && (
                <div className="lost-scan-panel">
                  <div className="scan-actions">
                    <p>Search the rest of this image for lost NTFS and ext4 volume starts.</p>
                    <button className="browse-button inventory-refresh" onClick={findLostVolumes} disabled={lostScanLoading} type="button">
                      {lostScanLoading ? 'Searching…' : 'Find lost volumes'} <span aria-hidden="true">⌕</span>
                    </button>
                  </div>
                  {lostScanMessage && <p className="input-message" role="alert">{lostScanMessage}</p>}
                  {visibleLostImageScan && (
                    <div className="scan-results lost-scan-results" aria-live="polite">
                      <div className="scan-results-heading">
                        <strong>Lost-volume search {visibleLostImageScan.complete ? 'complete' : 'stopped early'}</strong>
                        <span>{visibleLostImageScan.candidates.length} candidates · {formatBytes(visibleLostImageScan.scannedBytes)} searched</span>
                      </div>
                      {visibleLostImageScan.candidates.length === 0 ? (
                        <p className="scan-empty">No supported filesystem starts were found outside known partitions.</p>
                      ) : visibleLostImageScan.candidates.map((candidate) => (
                        <article className="scan-result-row" key={`${candidate.table}-${candidate.index}`}>
                          <div className="scan-result-main">
                            <strong>{candidate.filesystem?.kind ?? 'Unknown filesystem'}</strong>
                            <span>Candidate at {formatBytes(candidate.offsetBytes)} · {candidate.table}</span>
                            {candidate.filesystem?.reasons.map((reason) => <small key={reason}>✓ {reason}</small>)}
                          </div>
                          <span className="scan-confidence">{candidate.filesystem?.confidence ?? 'unknown'} confidence</span>
                          {candidate.filesystem?.kind === 'NTFS' && (
                            <button className="source-select-button" type="button" disabled={deletedFileScanLoading} onClick={() => void scanDeletedEntries(candidate)}>
                              {deletedFileScanLoading ? 'Reading…' : 'List deleted files'}
                            </button>
                          )}
                        </article>
                      ))}
                      <p className="scan-limitation">Search checks 512-byte boundaries and supports NTFS and ext4 signatures. A candidate is not proof that files can be recovered.</p>
                    </div>
                  )}
                </div>
              )}

              <div className="image-panel-footer">
                <span><span className="tiny-check">✓</span> File stays on this device</span>
                <span>NO UPLOADS</span>
              </div>
            </section>

            <aside className="workflow-panel" aria-labelledby="workflow-heading">
              <div className="panel-kicker">RECOVERY PATH</div>
              <h2 id="workflow-heading">Three careful steps</h2>
              <ol className="workflow-list">
                <li className="workflow-current">
                  <span className="step-number">01</span>
                  <span className="step-copy"><strong>Choose an external drive</strong><small>Internal drives stay locked</small></span>
                  <span className="step-state">NOW</span>
                </li>
                <li>
                  <span className="step-number">02</span>
                  <span className="step-copy"><strong>Open a disk image</strong><small>Physical imaging is not ready yet</small></span>
                  <span className="step-state step-state-next">NEXT</span>
                </li>
                <li>
                  <span className="step-number">03</span>
                  <span className="step-copy"><strong>Recover photos and videos</strong><small>File recovery is still in development</small></span>
                  <span className="step-state step-state-later">LATER</span>
                </li>
              </ol>
              <div className="workflow-note">
                <span className="note-mark" aria-hidden="true">↳</span>
                <p>Overwritten data cannot be reconstructed. Recovery is never guaranteed.</p>
              </div>
            </aside>
          </div>

          <section className="recovery-browser" aria-labelledby="recovery-browser-heading">
            <header className="recovery-browser-heading">
              <div>
                <div className="panel-kicker">FILE RECOVERY / 03</div>
                <h2 id="recovery-browser-heading">Photos and videos</h2>
              </div>
              <span className="feature-status">NTFS METADATA LISTING · EXPORT IN DEVELOPMENT</span>
            </header>
            <div className="recovery-browser-body">
              <aside className="recovery-filters" aria-label="File result filters">
                <div className="filter-mode" aria-label="Browse mode">
                  <span>Type</span><span>Path</span>
                </div>
                <button className={`file-category${fileCategory === 'photos' ? ' file-category-active' : ''}`} onClick={() => setFileCategory('photos')} type="button"><span aria-hidden="true">▧</span><strong>Photos</strong><small>{visibleDeletedFileScan?.entries.filter((entry) => entry.name && /\.(jpe?g|png|gif|bmp|tiff?|heic|webp)$/i.test(entry.name)).length ?? '—'}</small></button>
                <button className={`file-category${fileCategory === 'videos' ? ' file-category-active' : ''}`} onClick={() => setFileCategory('videos')} type="button"><span aria-hidden="true">▷</span><strong>Videos</strong><small>{visibleDeletedFileScan?.entries.filter((entry) => entry.name && /\.(mp4|mov|avi|mkv|mts|m4v|3gp|wmv)$/i.test(entry.name)).length ?? '—'}</small></button>
                <button className={`file-category${fileCategory === 'all' ? ' file-category-active' : ''}`} onClick={() => setFileCategory('all')} type="button"><span aria-hidden="true">▤</span><strong>All files</strong><small>{visibleDeletedFileScan?.entries.length ?? '—'}</small></button>
              </aside>
              <div className="recovery-results-pane">
                <div className="results-toolbar">
                  <div>
                    <strong>Recovered file list</strong>
                  <span>{visibleDeletedFileScan
                      ? `Volume at ${formatBytes(visibleDeletedFileScan.partitionOffsetBytes)} · ${filteredDeletedEntries.length} entries · metadata only`
                      : visibleLostImageScan
                        ? `${visibleLostImageScan.candidates.length} volume candidates · select an NTFS partition to list files`
                        : visibleImageScan
                          ? `${visibleImageScan.partitions.length} partitions checked · select an NTFS partition to list files`
                          : 'No image scan yet'}</span>
                  </div>
                  <button className="filter-button" type="button" disabled>Filter <span aria-hidden="true">⌄</span></button>
                </div>
                {deletedFileScanMessage && <p className="input-message" role="alert">{deletedFileScanMessage}</p>}
                {visibleDeletedFileScan ? (
                  <div className="deleted-file-list" aria-live="polite">
                    {filteredDeletedEntries.length === 0 ? (
                      <p className="scan-empty">No deleted file names in this category were found in the scanned MFT records.</p>
                    ) : filteredDeletedEntries.map((entry) => (
                      <article className="deleted-file-row" key={`${entry.recordNumber}-${entry.sequenceNumber}`}>
                        <span className="deleted-file-icon" aria-hidden="true">{entry.name?.match(/\.(mp4|mov|avi|mkv|mts|m4v|3gp|wmv)$/i) ? '▷' : '▧'}</span>
                        <span className="deleted-file-name"><strong>{entry.name}</strong><small>NTFS record {entry.recordNumber} · parent record {entry.parentRecordNumber ?? 'unknown'}</small></span>
                        <span className="deleted-file-size">{entry.dataSizeBytes === null ? 'Size unavailable' : formatBytes(entry.dataSizeBytes)}</span>
                        <span className="deleted-file-state">Deleted · content not read</span>
                      </article>
                    ))}
                    <p className="scan-limitation">Names and sizes come from surviving NTFS metadata. File contents and original folder paths are not available yet.</p>
                  </div>
                ) : <div className="results-search">
                  <span aria-hidden="true">⌕</span>
                  <input aria-label="Search recovered files" placeholder="Search recovered files" disabled />
                </div>}
                {!visibleDeletedFileScan && <div className="file-results-empty" aria-live="polite">
                  <div className="empty-art" aria-hidden="true">
                    <span className="empty-art-back" />
                    <span className="empty-art-front"><i /><i /><i /></span>
                    <span className="empty-art-lens" />
                  </div>
                  <h3>{selectedImage ? 'Select an NTFS volume to list deleted names' : 'Start with a disk image'}</h3>
                  <p>{selectedImage
                    ? 'Run a partition scan or find lost volumes, then choose “List deleted files” on an NTFS result. File contents and export are still in development.'
                    : 'Select an external source above, then open an existing disk image. Physical imaging is not available in this preview.'}</p>
                  {visibleLostImageScan && (
                    <div className="volume-summary">
                      <span className="summary-pulse" />
                      <span>Volume search {visibleLostImageScan.complete ? 'finished' : 'stopped'} · {visibleLostImageScan.candidates.length} candidates</span>
                    </div>
                  )}
                  {lostScanLoading && lostScanProgress && (
                    <div className="browser-scan-progress" aria-live="polite">
                      <div><span>Searching image regions</span><strong>{lostScanPercent}%</strong></div>
                      <progress max="100" value={lostScanPercent} aria-label="Image volume search progress" />
                      <small>{formatBytes(lostScanProgress.scannedBytes)} of {formatBytes(lostScanProgress.totalBytes)} checked</small>
                      <button className="text-button" onClick={cancelLostSearch} type="button">Cancel search</button>
                    </div>
                  )}
                </div>}
                <footer className="recovery-action-bar">
                  <div><span className="destination-mark" aria-hidden="true">↗</span><span><strong>Recovery destination</strong><small>Choose a separate drive when file export is ready.</small></span></div>
                  <button type="button" disabled>Recover selected</button>
                </footer>
              </div>
            </div>
          </section>

          <section className="build-note" aria-label="Preview build status">
            <span className="build-note-mark" aria-hidden="true">i</span>
            <p><strong>Preview build.</strong> Image inspection and NTFS deleted-name listing are available. Physical imaging and file-content export are still in development.</p>
            <span className="build-note-id">MILESTONE 05 / IMAGE SCAN</span>
          </section>
        </div>

        <footer className="footer">
          <span>DISKRESCUE <span className="footer-separator">/</span> RECOVERY WORKSPACE</span>
          <span>YOUR IMAGE AND RECOVERED FILES ARE SENSITIVE DATA.</span>
        </footer>
      </section>
    </main>
  );
}

export default App;
