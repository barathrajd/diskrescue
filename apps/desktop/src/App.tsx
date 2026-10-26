import { useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { cancelLostFilesystemSearch, listPhysicalSources, scanImage, searchLostFilesystems, useDiskImageSelection, type ImageScan, type LostImageScan, type LostScanProgress, type PhysicalSource } from './hooks/useDiskImageSelection';

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
  const [sourceMessage, setSourceMessage] = useState('');
  const [sourcesLoading, setSourcesLoading] = useState(false);
  const [imageScan, setImageScan] = useState<{ path: string; result: ImageScan } | null>(null);
  const [scanMessage, setScanMessage] = useState('');
  const [scanLoading, setScanLoading] = useState(false);
  const [lostImageScan, setLostImageScan] = useState<{ path: string; result: LostImageScan } | null>(null);
  const [lostScanProgress, setLostScanProgress] = useState<LostScanProgress | null>(null);
  const [lostScanMessage, setLostScanMessage] = useState('');
  const [lostScanLoading, setLostScanLoading] = useState(false);

  async function refreshPhysicalSources() {
    setSourceMessage('');
    setSourcesLoading(true);
    try {
      setPhysicalSources(await listPhysicalSources());
    } catch (error) {
      setSourceMessage(error instanceof Error ? error.message : 'Could not list physical disks.');
    } finally {
      setSourcesLoading(false);
    }
  }

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

  const visibleImageScan =
    imageScan && selectedImage && imageScan.path === selectedImage.path
      ? imageScan.result
      : null;
  const visibleLostImageScan =
    lostImageScan && selectedImage && lostImageScan.path === selectedImage.path
      ? lostImageScan.result
      : null;
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
              <h1>Recover from a copy.<br /><em>Keep the original safe.</em></h1>
              <p className="hero-description">
                DiskRescue works from a disk image so the affected drive stays untouched.
                Start with an image you have already created.
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
              <strong>Do not scan the affected drive directly.</strong>
              <span>Create an image with a recovery tool such as GNU ddrescue, then select that image here.</span>
            </div>
            <span className="safety-tag">SOURCE PROTECTED</span>
          </section>

          <div className="work-grid">
            <section className="image-panel" aria-labelledby="image-heading">
              <div className="panel-heading">
                <div>
                  <div className="panel-kicker">SOURCE / 01</div>
                  <h2 id="image-heading">Choose a disk image</h2>
                </div>
                <span className="panel-status"><span /> IMAGE INPUT</span>
              </div>

              {!selectedImage ? (
                <button className="dropzone" onClick={chooseImage} type="button">
                  <span className="drop-icon" aria-hidden="true">
                    <svg viewBox="0 0 32 32" fill="none">
                      <path d="M16 21V6m0 0-6 6m6-6 6 6M6 20v6h20v-6" />
                    </svg>
                  </span>
                  <span className="drop-title">Select an image file</span>
                  <span className="drop-subtitle">Raw disk images · IMG, DD, RAW, DMG, ISO</span>
                  <span className="browse-button">Browse files <span aria-hidden="true">↗</span></span>
                </button>
              ) : (
                <div className="selected-file" aria-live="polite">
                  <div className="file-emblem" aria-hidden="true">IMG</div>
                  <div className="file-details">
                    <strong title={selectedImage.name}>{selectedImage.name}</strong>
                    <span>{imageReview ? formatBytes(imageReview.imageBytes) : 'Selected locally'} <i /> Read-only inspection</span>
                  </div>
                  <button className="text-button" onClick={chooseImage} type="button">Choose another</button>
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
                  {lostScanLoading && lostScanProgress && (
                    <div className="lost-scan-progress" aria-live="polite">
                      <div><span>Scanning {formatBytes(lostScanProgress.scannedBytes)} of {formatBytes(lostScanProgress.totalBytes)}</span><span>{lostScanPercent}%</span></div>
                      <progress max="100" value={lostScanPercent} aria-label="Lost volume search progress" />
                      <button className="text-button" onClick={cancelLostSearch} type="button">Cancel search</button>
                    </div>
                  )}
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
                  <span className="step-copy"><strong>Choose an image</strong><small>Use a copy of the affected drive</small></span>
                  <span className="step-state">NOW</span>
                </li>
                <li>
                  <span className="step-number">02</span>
                  <span className="step-copy"><strong>Inspect partitions</strong><small>Read-only scan, no repairs</small></span>
                  <span className="step-state step-state-next">NEXT</span>
                </li>
                <li>
                  <span className="step-number">03</span>
                  <span className="step-copy"><strong>Recover to another drive</strong><small>Never export to the source</small></span>
                  <span className="step-state step-state-later">LATER</span>
                </li>
              </ol>
              <div className="workflow-note">
                <span className="note-mark" aria-hidden="true">↳</span>
                <p>Overwritten data cannot be reconstructed. Recovery is never guaranteed.</p>
              </div>
            </aside>
          </div>

          <section className="source-inventory" aria-labelledby="physical-sources-heading">
            <div className="inventory-heading">
              <div>
                <div className="panel-kicker">SOURCE / 02 · METADATA ONLY</div>
                <h2 id="physical-sources-heading">Physical disks</h2>
                <p>View device identity and mount state. This step does not read disk contents.</p>
              </div>
              <button className="browse-button inventory-refresh" onClick={refreshPhysicalSources} disabled={sourcesLoading} type="button">
                {sourcesLoading ? 'Checking…' : 'List disks'} <span aria-hidden="true">↻</span>
              </button>
            </div>
            {sourceMessage && <p className="input-message" role="alert">{sourceMessage}</p>}
            {physicalSources && (physicalSources.length === 0 ? (
              <p className="inventory-empty" aria-live="polite">No physical disks were reported by this system.</p>
            ) : (
              <div className="source-list" aria-live="polite">
                {physicalSources.map((source) => (
                  <article className="source-row" key={source.id}>
                    <div className="source-device-icon" aria-hidden="true">▤</div>
                    <div className="source-main">
                      <strong>{source.model ?? 'Model unavailable'}</strong>
                      <span>{source.id} · {formatBytes(source.capacityBytes)}</span>
                      <small>Serial: {source.serial ?? 'Unavailable'} · {source.isSystem ? 'System disk' : source.systemStateKnown ? 'Not system disk' : 'System state unknown'} · {source.mountPoints.length ? `Mounted at ${source.mountPoints.join(', ')}` : source.mountStateKnown ? 'No mounted volumes found' : 'Mount state unknown'}</small>
                    </div>
                    <div className={source.eligibilityReason ? 'source-eligibility blocked' : 'source-eligibility'}>
                      <span>{source.eligibilityReason ?? 'Metadata checks passed'}</span>
                      <small>Imaging helper not yet available</small>
                    </div>
                  </article>
                ))}
              </div>
            ))}
          </section>

          <section className="build-note" aria-label="Preview build status">
            <span className="build-note-mark" aria-hidden="true">i</span>
            <p><strong>Preview build.</strong> Image selection, partition-boundary NTFS/ext4 checks and physical disk metadata listing are ready. Lost-partition search, physical imaging and file recovery are still in development.</p>
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
