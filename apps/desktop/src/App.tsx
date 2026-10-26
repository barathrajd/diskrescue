import { useEffect, useState } from 'react';
import { listPhysicalSources, scanSelectedNtfsPartition, type DeletedFileEntry, type PhysicalSource } from './hooks/useDiskImageSelection';

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
  const [activePage, setActivePage] = useState<'drives' | 'partitions' | 'recovery'>('drives');
  const [physicalSources, setPhysicalSources] = useState<PhysicalSource[] | null>(null);
  const [selectedSourceId, setSelectedSourceId] = useState<string | null>(null);
  const [selectedPartitionId, setSelectedPartitionId] = useState<string | null>(null);
  const [sourceMessage, setSourceMessage] = useState('');
  const [sourcesLoading, setSourcesLoading] = useState(false);
  const [deletedFileScan, setDeletedFileScan] = useState<{ sourceId: string; partitionId: string; entries: DeletedFileEntry[] } | null>(null);
  const [deletedFileScanMessage, setDeletedFileScanMessage] = useState('');
  const [deletedFileScanLoading, setDeletedFileScanLoading] = useState(false);
  const [fileCategory, setFileCategory] = useState<'photos' | 'videos' | 'all'>('all');
  const selectedSource = physicalSources?.find((source) => source.id === selectedSourceId) ?? null;
  const selectedPartition = selectedSource?.partitions.find((partition) => partition.id === selectedPartitionId) ?? null;
  const partitionScanReady = Boolean(selectedSource && selectedPartition
    && selectedSource.externalStateKnown && selectedSource.isExternal
    && selectedSource.systemStateKnown && !selectedSource.isSystem
    && selectedSource.mountStateKnown && selectedSource.mountPoints.length === 0
    && selectedPartition.mountPoints.length === 0
    && selectedPartition.offsetBytes !== null && selectedPartition.sizeBytes > 0);

  function openSource(sourceId: string) {
    setSelectedSourceId(sourceId);
    setSelectedPartitionId(null);
    setDeletedFileScan(null);
    setActivePage('partitions');
  }

  function openPartition(partitionId: string) {
    setSelectedPartitionId(partitionId);
    setActivePage('recovery');
  }

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

  async function scanDeletedEntries() {
    if (!selectedSource || !selectedPartition) return;
    setDeletedFileScanMessage('');
    setDeletedFileScanLoading(true);
    try {
      const entries = await scanSelectedNtfsPartition(selectedSource, selectedPartition);
      setDeletedFileScan({ sourceId: selectedSource.id, partitionId: selectedPartition.id, entries });
    } catch (error) {
      setDeletedFileScan(null);
      setDeletedFileScanMessage(error instanceof Error ? error.message : 'Could not read deleted NTFS entries.');
    } finally {
      setDeletedFileScanLoading(false);
    }
  }

  const visibleDeletedFileScan = deletedFileScan && selectedSource && selectedPartition
    && deletedFileScan.sourceId === selectedSource.id && deletedFileScan.partitionId === selectedPartition.id
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
          <span>Drive recovery</span>
          <span className="nav-dot" />
        </div>

        <div className="rail-bottom">
          <div className="offline-mark"><span /> OFFLINE MODE</div>
          <div className="rail-version">BUILD 0.1.0 · PREVIEW</div>
        </div>
      </aside>

      <section className="workspace" id="top">
        <header className="topbar">
          <div className="breadcrumb"><span>WORKSPACE</span><b>/</b>{activePage !== 'drives' && <><button type="button" onClick={() => setActivePage('drives')}>EXTERNAL DRIVES</button><b>/</b></>}{activePage === 'recovery' ? 'RECOVERY' : activePage === 'partitions' ? 'PARTITIONS' : 'EXTERNAL DRIVES'}</div>
          <div className="topbar-status"><span className="status-light" /> READ-ONLY SESSION</div>
        </header>

        <div className="content">
          <div className="hero-grid">
            <div className="hero-copy">
              <div className="eyebrow"><span className="eyebrow-line" /> {activePage === 'drives' ? 'A CAREFUL WAY BACK' : activePage === 'partitions' ? 'DRIVE / PARTITIONS' : 'PARTITION / RECOVERY'}</div>
              <h1>{activePage === 'drives' ? <>Choose the external drive first.<br /><em>Keep it untouched.</em></> : activePage === 'partitions' ? <>Review its partitions.<br /><em>Keep the source safe.</em></> : <>Scan this partition.<br /><em>Keep it untouched.</em></>}</h1>
              <p className="hero-description">
                {activePage === 'drives'
                  ? 'Select an external source to load its partition list. Internal drives stay hidden.'
                  : activePage === 'partitions'
                    ? `Partition information for ${selectedSource?.model ?? selectedSource?.id ?? 'the selected drive'} comes from device metadata only.`
                    : `Selected partition: ${selectedPartition?.name ?? selectedPartition?.id ?? 'unknown'}. Recovery operations are kept on this separate page.`}
              </p>
            </div>
            <div className="hero-index" aria-hidden="true">
              <span className="hero-index-number">01</span>
              <span className="hero-index-rule" />
              <span className="hero-index-caption">READ-ONLY<br />BY DESIGN</span>
            </div>
          </div>

          {activePage !== 'drives' && <button className="page-back-button" type="button" onClick={() => setActivePage(activePage === 'recovery' ? 'partitions' : 'drives')}>
            ← {activePage === 'recovery' ? 'Back to partitions' : 'Back to external drives'}
          </button>}

          <section className="safety-strip" aria-label="Safety requirements">
            <div className="safety-icon" aria-hidden="true">!</div>
            <div className="safety-copy">
              <strong>Keep the affected partition unmounted.</strong>
                <span>{activePage === 'drives' ? 'DiskRescue keeps internal drives locked. Choose an external drive to review its partitions.' : 'DiskRescue reads the selected external partition directly in read-only mode. It will not mount, repair, or write to it.'}</span>
            </div>
            <span className="safety-tag">SOURCE PROTECTED</span>
          </section>

          {activePage === 'drives' && <section className="source-inventory" aria-labelledby="physical-sources-heading">
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
                    <article
                      className={`source-row source-row-clickable${isSelected ? ' source-row-selected' : ''}`}
                      key={source.id}
                      role="button"
                      tabIndex={0}
                      aria-pressed={isSelected}
                      onClick={() => openSource(source.id)}
                      onKeyDown={(event) => {
                        if (event.key === 'Enter' || event.key === ' ') {
                          event.preventDefault();
                          openSource(source.id);
                        }
                      }}
                    >
                      <div className="source-device-icon" aria-hidden="true">▤</div>
                      <div className="source-main">
                        <strong>{source.model ?? 'Model unavailable'}</strong>
                        <span>{source.id} · {formatBytes(source.capacityBytes)}</span>
                        <small>Serial: {source.serial ?? 'Unavailable'} · {source.isSystem ? 'System disk' : source.systemStateKnown ? 'Not system disk' : 'System state unknown'} · {source.mountPoints.length ? `Mounted at ${source.mountPoints.join(', ')}` : source.mountStateKnown ? 'No mounted volumes found' : 'Mount state unknown'}</small>
                      </div>
                      <div className={source.eligibilityReason ? 'source-eligibility blocked' : 'source-eligibility'}>
                        <span>{source.eligibilityReason ?? 'External drive detected'}</span>
                        <small>Partition scan available when unmounted</small>
                      </div>
                      <span className="source-open-label">{isSelected ? 'Open partitions ↗' : 'View partitions ↗'}</span>
                    </article>
                  );
                })}
              </div>
            ))}
            {selectedSourceId && (
              <p className="source-safety-note">Drive selected. Choose one of its partitions to open the scan workspace.</p>
            )}
            <p className="source-safety-note">Do not mount or repair the original. Partition details below come from device metadata only.</p>
          </section>}

          {activePage === 'partitions' && selectedSource && (
            <section className="source-inventory partition-page" aria-labelledby="partition-page-heading">
              <div className="inventory-heading">
                <div>
                  <div className="panel-kicker">{selectedSource.id} · {formatBytes(selectedSource.capacityBytes)}</div>
                  <h2 id="partition-page-heading">{selectedSource.model ?? 'External drive'}</h2>
                  <p>Choose a partition to open its recovery workspace. This list is device metadata only.</p>
                </div>
                <span className="source-page-status">SOURCE READ-ONLY</span>
              </div>
              {selectedSource.partitions.length === 0 ? (
                <p className="inventory-empty">No partitions were reported for this drive.</p>
              ) : (
                <div className="partition-list" aria-label="External drive partitions">
                  {selectedSource.partitions.map((partition) => (
                    <button className="partition-row partition-row-open" key={partition.id} type="button" onClick={() => openPartition(partition.id)}>
                      <span className="partition-icon" aria-hidden="true">▤</span>
                      <span className="partition-main"><strong>{partition.name ?? partition.id}</strong><small>{partition.id} · {partition.kind ?? 'Filesystem unknown'}</small></span>
                      <span className="partition-size">{formatBytes(partition.sizeBytes)}</span>
                      <span className="partition-mount">{partition.mountPoints.length ? `Mounted: ${partition.mountPoints.join(', ')}` : 'Not mounted'}</span>
                      <span className="partition-open-arrow" aria-hidden="true">↗</span>
                    </button>
                  ))}
                </div>
              )}
              <p className="partition-safety-note">Selecting a partition opens its workspace. DiskRescue reads the selected external partition directly and read-only; it will refuse mounted or internal drives.</p>
            </section>
          )}

          {activePage === 'recovery' && <>
          <div className="active-partition-banner">
            <span className="partition-icon" aria-hidden="true">▤</span>
            <span><small>SELECTED PARTITION</small><strong>{selectedPartition?.name ?? selectedPartition?.id}</strong><em>{selectedPartition?.id} · {selectedPartition?.kind ?? 'Filesystem needs identification'} · {formatBytes(selectedPartition?.sizeBytes ?? 0)}</em></span>
            <button className="text-button" type="button" onClick={() => setActivePage('partitions')}>Change partition</button>
          </div>
          <div className="work-grid">
            <section className="image-panel" aria-labelledby="image-heading">
              <div className="panel-heading">
                <div>
                  <div className="panel-kicker">READ-ONLY SCAN / SELECTED PARTITION</div>
                  <h2 id="image-heading">Scan this partition</h2>
                </div>
                <span className="panel-status"><span /> SOURCE READ-ONLY</span>
              </div>

              <div className="scan-actions direct-scan-actions">
                <p>Read NTFS metadata directly from the selected, unmounted external partition. No mount, repair, or source write is performed.</p>
                <button className="browse-button inventory-refresh" onClick={() => void scanDeletedEntries()} disabled={!partitionScanReady || deletedFileScanLoading} type="button">
                  {deletedFileScanLoading ? 'Scanning partition…' : 'Scan partition'} <span aria-hidden="true">⌕</span>
                </button>
              </div>
              {selectedSource && selectedPartition && !partitionScanReady && (
                <p className="partition-safety-note" role="status">To scan, DiskRescue must confirm this is an external non-system drive and the selected partition is unmounted.</p>
              )}
              {deletedFileScanMessage && <p className="input-message" role="alert">{deletedFileScanMessage}</p>}
              {visibleDeletedFileScan && (
                <div className="scan-results" aria-live="polite">
                  <div className="scan-results-heading">
                    <strong>NTFS metadata scan complete</strong>
                    <span>{visibleDeletedFileScan.entries.length} deleted entries · read-only</span>
                  </div>
                  <p className="scan-limitation">This scan lists names and metadata. File contents and folder paths are not recovered yet.</p>
                </div>
              )}

              <div className="image-panel-footer">
                <span><span className="tiny-check">✓</span> Source is read-only</span>
                <span>NO UPLOADS</span>
              </div>
            </section>

            <aside className="workflow-panel" aria-labelledby="workflow-heading">
              <div className="panel-kicker">RECOVERY PATH</div>
              <h2 id="workflow-heading">Recovery workspace</h2>
              <ol className="workflow-list">
                <li className="workflow-current">
                  <span className="step-number">01</span>
                  <span className="step-copy"><strong>Choose a partition</strong><small>{selectedPartition?.id ?? 'Partition selected'}</small></span>
                  <span className="step-state">NOW</span>
                </li>
                <li>
                  <span className="step-number">02</span>
                  <span className="step-copy"><strong>Scan the partition</strong><small>Read-only access · unmounted source</small></span>
                  <span className="step-state step-state-next">NEXT</span>
                </li>
                <li>
                  <span className="step-number">03</span>
                  <span className="step-copy"><strong>Review deleted names</strong><small>Export is not available yet</small></span>
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
                  <strong>Deleted file list</strong>
                    <span>{visibleDeletedFileScan
                      ? `${filteredDeletedEntries.length} entries in this view · NTFS metadata only`
                      : 'Scan the selected partition to list deleted names'}</span>
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
                  <h3>Scan the selected partition</h3>
                  <p>Click <strong>Scan partition</strong> above to read the unmounted external partition directly. DiskRescue will open it read-only; recovery contents and export are still in development.</p>
                </div>}
                <footer className="recovery-action-bar">
                  <div><span className="destination-mark" aria-hidden="true">↗</span><span><strong>Recovery destination</strong><small>Choose a separate drive when file export is ready.</small></span></div>
                  <button type="button" disabled>Recover selected</button>
                </footer>
              </div>
            </div>
          </section>
          </>}

          <section className="build-note" aria-label="Preview build status">
            <span className="build-note-mark" aria-hidden="true">i</span>
            <p><strong>Preview build.</strong> External unmounted NTFS partitions can be scanned read-only for deleted names. File-content recovery and export are still in development.</p>
            <span className="build-note-id">MILESTONE 05 / PARTITION SCAN</span>
          </section>
        </div>

        <footer className="footer">
          <span>DISKRESCUE <span className="footer-separator">/</span> RECOVERY WORKSPACE</span>
          <span>YOUR DRIVE AND RECOVERED DATA ARE SENSITIVE.</span>
        </footer>
      </section>
    </main>
  );
}

export default App;
