import { useDiskImageSelection } from './hooks/useDiskImageSelection';

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

          <section className="build-note" aria-label="Preview build status">
            <span className="build-note-mark" aria-hidden="true">i</span>
            <p><strong>Preview build.</strong> Image selection and basic MBR/GPT inspection are ready. Physical imaging and file recovery are still in development.</p>
            <span className="build-note-id">MILESTONE 01 / FOUNDATION</span>
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
