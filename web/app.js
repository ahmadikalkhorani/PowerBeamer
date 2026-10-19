// Presenter & Beamer Presenter - Exact 1:1 Slint GUI Logic
(function () {
  let decksData = [];
  let currentDeck = null;
  let currentSlideIndex = 0; // Starts on Slide 1 ("Dual-Screen LaTeX Beamer Slide Deck")
  let splitRatio = 0.32; // Default 32%
  let activeTool = 'pen'; // Fountain pen active
  let activeColor = '#fef04e'; // Yellow active
  let timerSeconds = 51; // 00:51 (matching user desktop screenshot)
  let isTimerRunning = true;
  let isDrawerOpen = false; // Drawer closed by default (matching user desktop screenshot)

  // Canvas drawing state
  const canvas = document.getElementById('ink-canvas');
  const ctx = canvas.getContext('2d');
  let isDrawing = false;
  let strokes = [];
  let currentStroke = [];

  // DOM Elements
  const docTitleText = document.getElementById('doc-title-text');
  const slideCounterText = document.getElementById('slide-counter-text');
  const timerElapsedText = document.getElementById('timer-elapsed-text');
  const stageSlideImg = document.getElementById('stage-slide-img');
  const drawerNoteImg = document.getElementById('drawer-note-img');
  const previewBadgeTitle = document.getElementById('preview-badge-title');
  const previewTagSplit = document.getElementById('preview-tag-split');
  const commentTextInput = document.getElementById('comment-text-input');
  const commentCharCounter = document.getElementById('comment-char-counter');

  // Media Player Toolbar Elements
  const mediaToolbarOverlay = document.getElementById('media-toolbar-overlay');
  const mediaBtnPlay = document.getElementById('media-btn-play');
  const mediaPlayIcon = document.getElementById('media-play-icon');
  const mediaBtnStop = document.getElementById('media-btn-stop');
  const mediaTimeReadout = document.getElementById('media-time-readout');
  const mediaScrubberTrack = document.getElementById('media-scrubber-track');
  const mediaScrubberFill = document.getElementById('media-scrubber-fill');
  const mediaScrubberThumb = document.getElementById('media-scrubber-thumb');
  const mediaBtnSpeech = document.getElementById('media-btn-speech');
  const mediaSpeechIcon = document.getElementById('media-speech-icon');
  const mediaBtnSpeed = document.getElementById('media-btn-speed');
  const mediaSpeedText = document.getElementById('media-speed-text');
  const mediaClipName = document.getElementById('media-clip-name');
  const ytAutoplayBtn = document.getElementById('yt-autoplay-btn');
  const ytAutoplayKnob = document.getElementById('yt-autoplay-knob');
  const ytBtnCc = document.getElementById('yt-btn-cc');
  const ytBtnMiniplayer = document.getElementById('yt-btn-miniplayer');
  const ytBtnFullscreen = document.getElementById('yt-btn-fullscreen');
  const inplaceVideoBox = document.getElementById('inplace-video-box');
  const inplaceVideoPlayer = document.getElementById('inplace-video-player');
  const stageArea = document.getElementById('stage-area');
  const slintWorkspace = document.querySelector('.slint-workspace');

  // Media Playback State
  let isMediaPlaying = false;
  let mediaCurrentTime = 0;
  let mediaTotalDuration = 6.0; // Probe duration or default 6.0
  let mediaSpeed = 1.0;
  let isMediaSpeech = true;
  let isAutoplayEnabled = true;
  let isCcEnabled = false;
  let isScrubbing = false;

  // Sliders and percentage labels
  const drawerRatioSlider = document.getElementById('drawer-ratio-slider');
  const topRatioSlider = document.getElementById('top-ratio-slider');
  const readoutSlidePct = document.getElementById('readout-slide-pct');
  const readoutNotesPct = document.getElementById('readout-notes-pct');
  const topSlidePct = document.getElementById('top-slide-pct');
  const topNotesPct = document.getElementById('top-notes-pct');
  const notesDrawer = document.getElementById('notes-drawer');
  const splitterBar = document.getElementById('splitter-bar');
  const topbarSplitSliderWrap = document.getElementById('topbar-split-slider-wrap');
  const btnNotesDrawer = document.getElementById('btn-notes-drawer');
  const btnCloseNotes = document.getElementById('btn-close-notes');
  const presetBtns = document.querySelectorAll('.slint-preset-chip');

  // Detached Window Modal
  const btnDetachWindowCta = document.getElementById('btn-detach-window-cta');
  const modalDetached = document.getElementById('modal-detached');
  const btnCloseDetachedModal = document.getElementById('btn-close-detached-modal');
  const detachedTextarea = document.getElementById('detached-textarea');
  const detachedNoteImg = document.getElementById('detached-note-img');
  const detachedSlideLabel = document.getElementById('detached-slide-label');
  const detachedTimerDisplay = document.getElementById('detached-timer-display');

  // Screen Controls, Beamer Frame & TOC Elements
  let blankMode = 0; // 0: Normal, 1: Blackout, 2: Whiteout
  let isScreenSwapped = false;
  let isTocOpen = false;

  const btnBlackout = document.getElementById('btn-blackout');
  const btnWhiteout = document.getElementById('btn-whiteout');
  const btnSwap = document.getElementById('btn-swap');
  const btnToc = document.getElementById('btn-toc');
  const btnCloseToc = document.getElementById('btn-close-toc');
  const tocDrawerOverlay = document.getElementById('toc-drawer-overlay');
  const tocDrawerBackdrop = document.getElementById('toc-drawer-backdrop');
  const tocItemsContainer = document.getElementById('toc-items-container');
  const frameCounterPill = document.getElementById('frame-counter-pill');
  const frameCounterText = document.getElementById('frame-counter-text');
  const btnPrevFrame = document.getElementById('btn-prev-frame');
  const btnNextFrame = document.getElementById('btn-next-frame');
  const stageBlankBanner = document.getElementById('stage-blank-banner');
  const stageBlankBannerText = document.getElementById('stage-blank-banner-text');
  const stageBlankBannerClose = document.getElementById('stage-blank-banner-close');
  const projectorBlankOverlay = document.getElementById('projector-blank-overlay');

  // Load Decks from JSON
  async function loadDecks() {
    try {
      const res = await fetch('assets/decks.json');
      decksData = await res.json();
      const urlParams = new URLSearchParams(window.location.search);
      const requestedDeckId = urlParams.get('deck') || urlParams.get('presentation') || urlParams.get('file');
      currentDeck = (requestedDeckId && decksData.find(d => d.id === requestedDeckId || d.file_name.includes(requestedDeckId)))
        || decksData.find(d => d.id === 'beamer_notes')
        || decksData.find(d => d.id === 'dual_screen')
        || decksData[0];
      currentSlideIndex = 0; // Start on Slide 1
      renderCurrentSlide();
      toggleDrawer(true);
      selectTool('pen');
      selectColor('#fef04e', slotYellow);

      // Deck switcher on close button and doc title
      const btnCloseDoc = document.getElementById('btn-close-doc');
      if (btnCloseDoc) {
        btnCloseDoc.title = 'Click to switch between loaded presentation decks';
        btnCloseDoc.addEventListener('click', () => {
          if (!decksData.length) return;
          const curIdx = decksData.findIndex(d => d.id === currentDeck.id);
          const nextIdx = (curIdx + 1) % decksData.length;
          currentDeck = decksData[nextIdx];
          currentSlideIndex = 0;
          renderCurrentSlide();
        });
      }
      if (docTitleText) {
        docTitleText.style.cursor = 'pointer';
        docTitleText.title = 'Click to switch presentation deck';
        docTitleText.addEventListener('click', () => {
          if (!decksData.length) return;
          const curIdx = decksData.findIndex(d => d.id === currentDeck.id);
          const nextIdx = (curIdx + 1) % decksData.length;
          currentDeck = decksData[nextIdx];
          currentSlideIndex = 0;
          renderCurrentSlide();
        });
      }
    } catch (e) {
      console.error('Failed to load decks.json:', e);
    }
  }

  function renderCurrentSlide() {
    if (!currentDeck || !currentDeck.slides.length) return;
    const slide = currentDeck.slides[currentSlideIndex];

    docTitleText.textContent = currentDeck.file_name;
    slideCounterText.textContent = `${currentSlideIndex + 1} / ${currentDeck.slides.length}`;
    detachedSlideLabel.textContent = `Slide ${currentSlideIndex + 1} of ${currentDeck.slides.length}`;

    // Update navigation arrows enabled/disabled states
    const btnPrev = document.getElementById('btn-prev-slide-arrow');
    const btnNext = document.getElementById('btn-next-slide-arrow');
    const stagePrev = document.getElementById('stage-arrow-prev');
    const stageNext = document.getElementById('stage-arrow-next');
    if (btnPrev) btnPrev.disabled = currentSlideIndex === 0;
    if (btnNext) btnNext.disabled = currentSlideIndex >= currentDeck.slides.length - 1;
    if (stagePrev) stagePrev.style.display = currentSlideIndex > 0 ? 'flex' : 'none';
    if (stageNext) stageNext.style.display = currentSlideIndex < currentDeck.slides.length - 1 ? 'flex' : 'none';

    // Beamer Logical Frame & Overlay Counter
    const curFrame = slide.frame_index || (currentSlideIndex + 1);
    const totFrames = slide.total_frames || (currentDeck.total_frames || currentDeck.slides.length);
    const overStep = slide.overlay_step || 1;
    const overTotal = slide.overlay_total || 1;
    if (frameCounterText) {
      if (overTotal > 1) {
        frameCounterText.textContent = `Frame ${curFrame}/${totFrames} • Step ${overStep}/${overTotal}`;
      } else {
        frameCounterText.textContent = `Frame ${curFrame}/${totFrames}`;
      }
    }

    // Set audience slide image vs Notes Preview (with Screen Swapping support)
    const audImgSrc = `assets/${slide.audience_img}`;
    const noteImgSrc = (slide.has_notes_image && slide.notes_img) ? `assets/${slide.notes_img}` : null;

    if (isScreenSwapped) {
      // Swapped: Laptop shows audience slide, Projector displays note/presenter view
      stageSlideImg.src = noteImgSrc || audImgSrc;
      drawerNoteImg.src = audImgSrc;
      detachedNoteImg.src = audImgSrc;
    } else {
      stageSlideImg.src = audImgSrc;
      if (noteImgSrc) {
        if (slide.is_dual_screen) {
          previewBadgeTitle.textContent = "📐 DUAL-SCREEN NOTE SLIDE (pgfpages)";
          previewTagSplit.style.display = "inline-block";
          previewTagSplit.textContent = "32:9 Split";
        } else {
          previewBadgeTitle.textContent = "📝 PRESENTER SPEAKER NOTE";
          previewTagSplit.style.display = "inline-block";
          previewTagSplit.textContent = "Note Page";
        }
        drawerNoteImg.src = noteImgSrc;
        detachedNoteImg.src = noteImgSrc;
      } else {
        previewBadgeTitle.textContent = "⏭ NEXT SLIDE PREVIEW";
        previewTagSplit.style.display = "none";
        const nextSlide = currentDeck.slides[currentSlideIndex + 1] || slide;
        drawerNoteImg.src = `assets/${nextSlide.audience_img}`;
        detachedNoteImg.src = `assets/${slide.audience_img}`;
      }
    }

    // Resize ink canvas
    resizeCanvas();

    // Populate comments / notes editor
    commentTextInput.value = slide.notes;
    detachedTextarea.value = slide.notes;

    // Update screen blank and outline UI
    updateBlankUI();
    renderTocDrawer();
    updateCharCounter();

    // Check media
    if (slide.media_clips && slide.media_clips.length > 0) {
      mediaToolbarOverlay.style.display = 'flex';
      if (slintWorkspace) slintWorkspace.classList.add('has-media');
      const clip = slide.media_clips[0];
      if (mediaClipName) mediaClipName.textContent = clip.display_path;
      const rect = clip.rect || [0.28904, 0.55650, 0.45103, 0.38324];
      if (inplaceVideoBox) {
        inplaceVideoBox.style.left = (rect[0] * 100) + '%';
        inplaceVideoBox.style.top = (rect[1] * 100) + '%';
        inplaceVideoBox.style.width = (rect[2] * 100) + '%';
        inplaceVideoBox.style.height = (rect[3] * 100) + '%';
        inplaceVideoBox.style.display = 'none';
      }
      if (inplaceVideoPlayer) {
        inplaceVideoPlayer.src = 'assets/' + clip.display_path;
        inplaceVideoPlayer.onloadedmetadata = () => {
          if (inplaceVideoPlayer.duration && !isNaN(inplaceVideoPlayer.duration)) {
            mediaTotalDuration = inplaceVideoPlayer.duration;
            updateMediaUI();
          }
        };
        inplaceVideoPlayer.ontimeupdate = () => {
          if (!isScrubbing && inplaceVideoPlayer.duration) {
            mediaCurrentTime = inplaceVideoPlayer.currentTime;
            updateMediaUI();
          }
        };
        inplaceVideoPlayer.onended = () => {
          isMediaPlaying = false;
          mediaCurrentTime = mediaTotalDuration;
          updateMediaUI();
          if (isAutoplayEnabled) {
            // Smoothly advance to next slide when autoplay is active
            if (currentSlideIndex < currentDeck.slides.length - 1) {
              setTimeout(() => {
                goToSlide(currentSlideIndex + 1);
              }, 1200);
            }
          }
        };
      }
      mediaTotalDuration = 6.0;
      updateMediaUI();
    } else {
      mediaToolbarOverlay.style.display = 'none';
      if (slintWorkspace) slintWorkspace.classList.remove('has-media');
      stopMedia();
    }
  }

  // --- MEDIA TOOLBAR CONTROLS (PLAY, STOP, TIME BAR, SPEECH, SPEED) ---
  function formatMediaTime(secs) {
    const s = Math.max(0, Math.floor(secs));
    const m = Math.floor(s / 60);
    const remS = s % 60;
    return `${m.toString().padStart(2, '0')}:${remS.toString().padStart(2, '0')}`;
  }

  function updateMediaUI() {
    if (!mediaTimeReadout) return;
    mediaTimeReadout.textContent = `${formatMediaTime(mediaCurrentTime)} / ${formatMediaTime(mediaTotalDuration)}`;
    const progress = Math.max(0, Math.min(1, mediaCurrentTime / (mediaTotalDuration || 1)));
    const pct = progress * 100;
    if (mediaScrubberFill) mediaScrubberFill.style.width = `${pct}%`;
    if (mediaScrubberThumb) mediaScrubberThumb.style.left = `${pct}%`;
    
    // Play/Pause/Replay icon with pure white SVG symbols (no background)
    if (mediaPlayIcon) {
      if (mediaTotalDuration > 0 && mediaCurrentTime >= mediaTotalDuration - 0.1) {
        mediaPlayIcon.src = 'icons/replay.svg';
        mediaPlayIcon.alt = 'Replay';
      } else {
        mediaPlayIcon.src = isMediaPlaying ? 'icons/pause.svg' : 'icons/play.svg';
        mediaPlayIcon.alt = isMediaPlaying ? 'Pause' : 'Play';
      }
    }
    if (mediaBtnPlay) {
      if (isMediaPlaying) {
        mediaBtnPlay.classList.add('playing');
      } else {
        mediaBtnPlay.classList.remove('playing');
      }
    }
  const speedOptions = [1.0, 1.25, 1.5, 2.0, 0.75];
  function formatSpeedText(s) {
    if (Math.abs(s - 1.0) < 0.01) return '1.0x';
    if (Math.abs(s - 1.25) < 0.01) return '1.25x';
    if (Math.abs(s - 1.5) < 0.01) return '1.5x';
    if (Math.abs(s - 2.0) < 0.01) return '2.0x';
    if (Math.abs(s - 0.75) < 0.01) return '0.75x';
    return `${s.toFixed(1)}x`;
  }

    if (mediaSpeedText) {
      mediaSpeedText.textContent = formatSpeedText(mediaSpeed);
    }
    if (mediaSpeechIcon) {
      mediaSpeechIcon.src = isMediaSpeech ? 'icons/volume.svg' : 'icons/mute.svg';
      mediaSpeechIcon.alt = isMediaSpeech ? 'Mute' : 'Unmute';
    }
    if (mediaBtnSpeech) {
      mediaBtnSpeech.classList.toggle('active', isMediaSpeech);
    }
    if (ytAutoplayKnob) {
      ytAutoplayKnob.textContent = isAutoplayEnabled ? '⏸' : '▶';
    }
  }

  function playMedia() {
    if (isMediaPlaying) return;
    isMediaPlaying = true;
    updateMediaUI();
    if (inplaceVideoBox) inplaceVideoBox.style.display = 'block';
    if (inplaceVideoPlayer) {
      inplaceVideoPlayer.playbackRate = mediaSpeed;
      inplaceVideoPlayer.muted = !isMediaSpeech;
      inplaceVideoPlayer.play().catch(e => {
        console.warn("In-place video playback autoplay notice:", e);
      });
    }
  }

  function pauseMedia() {
    isMediaPlaying = false;
    if (inplaceVideoPlayer) {
      inplaceVideoPlayer.pause();
    }
    updateMediaUI();
  }

  function toggleMedia() {
    if (mediaTotalDuration > 0 && mediaCurrentTime >= mediaTotalDuration - 0.1) {
      // Replay from beginning
      mediaCurrentTime = 0;
      if (inplaceVideoPlayer) inplaceVideoPlayer.currentTime = 0;
      playMedia();
      return;
    }
    if (isMediaPlaying) {
      pauseMedia();
    } else {
      playMedia();
    }
  }

  function stopMedia() {
    isMediaPlaying = false;
    if (inplaceVideoPlayer) {
      inplaceVideoPlayer.pause();
      inplaceVideoPlayer.currentTime = 0;
    }
    if (inplaceVideoBox) {
      inplaceVideoBox.style.display = 'none';
    }
    mediaCurrentTime = 0;
    updateMediaUI();
  }

  function seekMediaFromEvent(e) {
    const rect = mediaScrubberTrack.getBoundingClientRect();
    const clickX = e.clientX - rect.left;
    const ratio = Math.max(0, Math.min(1, clickX / rect.width));
    mediaCurrentTime = ratio * mediaTotalDuration;
    if (inplaceVideoPlayer) {
      inplaceVideoPlayer.currentTime = mediaCurrentTime;
      if (inplaceVideoBox) {
        inplaceVideoBox.style.display = 'block';
      }
    }
    updateMediaUI();
  }

  if (mediaBtnPlay) mediaBtnPlay.addEventListener('click', toggleMedia);
  if (mediaBtnStop) mediaBtnStop.addEventListener('click', stopMedia);
  if (inplaceVideoBox) inplaceVideoBox.addEventListener('click', toggleMedia);

  if (mediaScrubberTrack) {
    mediaScrubberTrack.addEventListener('mousedown', (e) => {
      isScrubbing = true;
      seekMediaFromEvent(e);
    });
  }

  window.addEventListener('mousemove', (e) => {
    if (isScrubbing) {
      seekMediaFromEvent(e);
    }
  });

  window.addEventListener('mouseup', () => {
    if (isScrubbing) {
      isScrubbing = false;
    }
  });

  if (mediaBtnSpeech) {
    mediaBtnSpeech.addEventListener('click', () => {
      isMediaSpeech = !isMediaSpeech;
      if (inplaceVideoPlayer) {
        inplaceVideoPlayer.muted = !isMediaSpeech;
      }
      updateMediaUI();
    });
  }

  if (mediaBtnSpeed) {
    mediaBtnSpeed.addEventListener('click', () => {
      const idx = speedOptions.findIndex(s => Math.abs(s - mediaSpeed) < 0.05);
      const nextIdx = (idx >= 0 ? idx + 1 : 0) % speedOptions.length;
      mediaSpeed = speedOptions[nextIdx];
      if (inplaceVideoPlayer) {
        inplaceVideoPlayer.playbackRate = mediaSpeed;
      }
      updateMediaUI();
    });
  }



  function updateNotes(text) {
    if (!currentDeck) return;
    currentDeck.slides[currentSlideIndex].notes = text;
    commentTextInput.value = text;
    detachedTextarea.value = text;
    updateCharCounter();
  }

  function updateCharCounter() {
    const len = commentTextInput.value.length;
    commentCharCounter.textContent = `${len} characters`;
  }

  commentTextInput.addEventListener('input', (e) => updateNotes(e.target.value));
  detachedTextarea.addEventListener('input', (e) => updateNotes(e.target.value));

  // Quick Action Buttons
  function insertSnippet(snippet) {
    const current = commentTextInput.value;
    const prefix = (current.length > 0 && !current.endsWith('\n')) ? '\n' : '';
    updateNotes(current + prefix + snippet);
  }

  document.getElementById('btn-add-bullet').addEventListener('click', () => insertSnippet('• '));
  document.getElementById('btn-add-reminder').addEventListener('click', () => insertSnippet('⏰ Reminder: '));
  document.getElementById('btn-clear-notes').addEventListener('click', () => updateNotes(''));

  document.getElementById('detached-btn-bullet').addEventListener('click', () => insertSnippet('• '));
  document.getElementById('detached-btn-reminder').addEventListener('click', () => insertSnippet('⏰ Reminder: '));
  document.getElementById('detached-btn-clear').addEventListener('click', () => updateNotes(''));

  // Split Ratio
  function setSplitRatio(ratioPct) {
    const ratio = Math.max(15, Math.min(70, Number(ratioPct)));
    splitRatio = ratio / 100;
    const notesPct = Math.round(ratio);

    if (notesDrawer) {
      notesDrawer.style.width = `${notesPct}%`;
    }

    resizeCanvas();
  }

  // Draggable Horizontal Splitter Slider Logic (between Note size preview and speaker notes)
  let noteSizeRatio = 0.30;
  const notesHSplitter = document.getElementById('notes-h-splitter');
  const previewImgContainer = document.getElementById('preview-img-container');

  function setNoteSizeRatio(ratio) {
    noteSizeRatio = Math.max(0.15, Math.min(0.70, ratio));

    // Scale preview-img-container height based on available height in drawer
    if (previewImgContainer) {
      const drawerHeight = notesDrawer ? notesDrawer.clientHeight : 700;
      const previewHeight = Math.max(80, Math.min(drawerHeight - 240, (drawerHeight - 120) * noteSizeRatio));
      previewImgContainer.style.height = `${Math.round(previewHeight)}px`;
    }
  }

  let isDraggingHSplitter = false;
  let startHSplitterY = 0;
  let startHSplitterRatio = 0.30;

  if (notesHSplitter) {
    notesHSplitter.addEventListener('mousedown', (e) => {
      isDraggingHSplitter = true;
      startHSplitterY = e.clientY;
      startHSplitterRatio = noteSizeRatio;
      notesHSplitter.classList.add('dragging');
      document.body.style.cursor = 'row-resize';
      e.preventDefault();
    });

    window.addEventListener('mousemove', (e) => {
      if (!isDraggingHSplitter) return;
      const deltaY = e.clientY - startHSplitterY;
      const drawerHeight = notesDrawer ? notesDrawer.clientHeight : 700;
      const deltaRatio = deltaY / Math.max(100, drawerHeight - 120);
      setNoteSizeRatio(startHSplitterRatio + deltaRatio);
    });

    window.addEventListener('mouseup', () => {
      if (isDraggingHSplitter) {
        isDraggingHSplitter = false;
        notesHSplitter.classList.remove('dragging');
        document.body.style.cursor = 'default';
      }
    });
  }

  // Detached Note Size Horizontal Splitter Logic
  const detachedNotesHSplitter = document.getElementById('detached-notes-h-splitter');
  let isDraggingDetachedH = false;
  let startDetachedHY = 0;
  let startDetachedHRatio = 0.32;

  function setDetachedNoteSizeRatio(ratio) {
    const r = Math.max(0.15, Math.min(0.70, ratio));
    if (detachedNoteImg) {
      const h = Math.max(90, Math.min(450, 600 * r));
      detachedNoteImg.style.height = `${Math.round(h)}px`;
    }
  }

  if (detachedNotesHSplitter) {
    detachedNotesHSplitter.addEventListener('mousedown', (e) => {
      isDraggingDetachedH = true;
      startDetachedHY = e.clientY;
      startDetachedHRatio = noteSizeRatio;
      detachedNotesHSplitter.classList.add('dragging');
      document.body.style.cursor = 'row-resize';
      e.preventDefault();
    });

    window.addEventListener('mousemove', (e) => {
      if (!isDraggingDetachedH) return;
      const deltaY = e.clientY - startDetachedHY;
      const deltaRatio = deltaY / 400;
      setDetachedNoteSizeRatio(startDetachedHRatio + deltaRatio);
    });

    window.addEventListener('mouseup', () => {
      if (isDraggingDetachedH) {
        isDraggingDetachedH = false;
        detachedNotesHSplitter.classList.remove('dragging');
        document.body.style.cursor = 'default';
      }
    });
  }

  // Splitter Dragging
  let isDraggingSplitter = false;
  splitterBar.addEventListener('mousedown', () => {
    isDraggingSplitter = true;
    splitterBar.classList.add('dragging');
    document.body.style.cursor = 'col-resize';
  });

  window.addEventListener('mousemove', (e) => {
    if (!isDraggingSplitter) return;
    const workspaceRect = document.querySelector('.slint-workspace').getBoundingClientRect();
    const rightOffset = workspaceRect.right - e.clientX;
    const ratioPct = (rightOffset / workspaceRect.width) * 100;
    setSplitRatio(ratioPct);
  });

  window.addEventListener('mouseup', () => {
    if (isDraggingSplitter) {
      isDraggingSplitter = false;
      splitterBar.classList.remove('dragging');
      document.body.style.cursor = 'default';
    }
  });

  // Drawer Toggle
  function toggleDrawer(force) {
    isDrawerOpen = force !== undefined ? force : !isDrawerOpen;
    if (isDrawerOpen) {
      notesDrawer.classList.remove('collapsed');
      splitterBar.style.display = 'block';
      btnNotesDrawer.classList.add('active');
    } else {
      notesDrawer.classList.add('collapsed');
      splitterBar.style.display = 'none';
      btnNotesDrawer.classList.remove('active');
    }
    resizeCanvas();
  }

  btnNotesDrawer.addEventListener('click', () => toggleDrawer());
  btnCloseNotes.addEventListener('click', () => toggleDrawer(false));

  // Detached Floating Window Modal
  btnDetachWindowCta.addEventListener('click', () => {
    modalDetached.style.display = 'flex';
  });
  btnCloseDetachedModal.addEventListener('click', () => {
    modalDetached.style.display = 'none';
  });

  // Toolbar Tools
  const toolBtns = {
    thumbnails: document.getElementById('tool-thumbnails'),
    pen: document.getElementById('tool-pen'),
    highlighter: document.getElementById('tool-highlighter'),
    eraser: document.getElementById('tool-eraser'),
    laser: document.getElementById('tool-laser'),
  };

  const svgCache = {};
  async function loadAndTintSvg(filename, replaceWhiteWith) {
    if (!svgCache[filename]) {
      try {
        const resp = await fetch(`icons/${filename}`);
        if (resp.ok) {
          svgCache[filename] = await resp.text();
        }
      } catch (err) {
        console.warn(`Could not load icon ${filename}:`, err);
      }
    }
    const raw = svgCache[filename];
    if (!raw) return null;
    if (!replaceWhiteWith || replaceWhiteWith.toLowerCase() === '#ffffff') {
      return `data:image/svg+xml;utf8,${encodeURIComponent(raw)}`;
    }
    // Replace white color with selected color in dark theme
    const tinted = raw.replaceAll('#ffffff', replaceWhiteWith).replaceAll('#FFFFFF', replaceWhiteWith);
    return `data:image/svg+xml;utf8,${encodeURIComponent(tinted)}`;
  }

  async function updateToolIcons() {
    const isThumbnailsActive = toolBtns.thumbnails ? toolBtns.thumbnails.classList.contains('active') : false;
    const toolMappings = [
      { key: 'thumbnails', file: 'thumbnails.svg', active: isThumbnailsActive },
      { key: 'pen', file: 'pen.svg', active: activeTool === 'pen' },
      { key: 'highlighter', file: 'highlighter.svg', active: activeTool === 'highlighter' },
      { key: 'eraser', file: 'eraser.svg', active: activeTool === 'eraser' },
      { key: 'laser', file: 'laser.svg', active: activeTool === 'laser' },
    ];

    for (const item of toolMappings) {
      const btn = toolBtns[item.key];
      if (!btn) continue;
      const img = btn.querySelector('img');
      if (!img) continue;

      const color = item.active ? activeColor : '#ffffff';
      const dataUri = await loadAndTintSvg(item.file, color);
      if (dataUri) {
        img.src = dataUri;
      }
    }
  }

  function updateCanvasCursor() {
    const canvas = document.getElementById('ink-canvas');
    if (!canvas) return;

    if (activeTool === 'pen') {
      const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none"><path d="M6 10 L16 20 L20 16 L10 6 Z" fill="#2563eb" stroke="#0f172a" stroke-width="1.4" stroke-linejoin="round"/><path d="M8 8 L10 6 L12 8 L10 10 Z" fill="#cbd5e1" stroke="#0f172a" stroke-width="1"/><path d="M2 2 L8 8 L5 11 L2 5 Z" fill="#f8fafc" stroke="#0f172a" stroke-width="1.4" stroke-linejoin="round"/><line x1="2" y1="2" x2="6" y2="6" stroke="#0f172a" stroke-width="1.2"/><circle cx="5.5" cy="5.5" r="0.8" fill="#0f172a"/><circle cx="2" cy="2" r="2.0" fill="${activeColor}" stroke="#0f172a" stroke-width="0.8"/></svg>`;
      canvas.style.cursor = `url("data:image/svg+xml,${encodeURIComponent(svg)}") 2 2, crosshair`;
    } else if (activeTool === 'highlighter') {
      const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none"><polygon points="6,9 10,5 21,16 17,20" fill="#eab308" stroke="#0f172a" stroke-width="1.4" stroke-linejoin="round"/><polygon points="17,20 21,16 23,18 19,22" fill="#64748b" stroke="#0f172a" stroke-width="1.2"/><polygon points="2,3 5,1 10,6 6,9" fill="${activeColor}" stroke="#0f172a" stroke-width="1.4" stroke-linejoin="round"/><circle cx="2" cy="2" r="1.8" fill="${activeColor}" stroke="#0f172a" stroke-width="0.8"/></svg>`;
      canvas.style.cursor = `url("data:image/svg+xml,${encodeURIComponent(svg)}") 2 2, crosshair`;
    } else if (activeTool === 'eraser') {
      canvas.style.cursor = 'url("icons/eraser_cursor.svg") 12 12, cell';
    } else if (activeTool === 'laser') {
      canvas.style.cursor = 'none';
    }
  }

  function selectTool(name) {
    activeTool = name;
    Object.keys(toolBtns).forEach(k => {
      if (k === name) toolBtns[k].classList.add('active');
      else if (k !== 'thumbnails') toolBtns[k].classList.remove('active');
    });

    const laserDot = document.getElementById('stage-laser-dot');
    if (laserDot) {
      laserDot.style.display = (name === 'laser') ? 'block' : 'none';
    }

    const workspace = document.querySelector('.slint-workspace');
    if (workspace) {
      workspace.setAttribute('data-active-tool', name);
    }

    updateCanvasCursor();
    updateToolIcons();
  }

  const pageOverviewOverlay = document.getElementById('page-overview-overlay');
  const overviewScrollContainer = document.getElementById('overview-scroll-container');
  let draggedSlideIndex = null;

  function renderPageOverview() {
    if (!currentDeck || !overviewScrollContainer) return;
    overviewScrollContainer.innerHTML = '';

    currentDeck.slides.forEach((slide, idx) => {
      const item = document.createElement('div');
      item.className = 'overview-thumb-item' + (idx === currentSlideIdx ? ' active' : '');
      item.draggable = true;
      item.dataset.slideIndex = idx;

      const card = document.createElement('div');
      card.className = 'overview-thumb-card';
      const img = document.createElement('img');
      img.src = 'assets/' + slide.audience_img;
      img.alt = `Slide ${idx + 1}`;
      card.appendChild(img);

      const dropHint = document.createElement('div');
      dropHint.className = 'overview-drop-hint';
      dropHint.innerHTML = '<span>⬇</span><span>Insert Here</span>';
      card.appendChild(dropHint);

      const badge = document.createElement('div');
      badge.className = 'overview-badge';
      badge.innerHTML = `<span class="overview-grip">⠿</span><span>${idx + 1}</span>`;

      item.appendChild(card);
      item.appendChild(badge);

      // Drag and drop event listeners
      item.addEventListener('dragstart', (e) => {
        draggedSlideIndex = idx;
        item.classList.add('dragging');
        e.dataTransfer.effectAllowed = 'move';
        e.dataTransfer.setData('text/plain', idx);
      });

      item.addEventListener('dragend', () => {
        item.classList.remove('dragging');
        document.querySelectorAll('.overview-thumb-item').forEach(el => el.classList.remove('drop-target'));
      });

      item.addEventListener('dragover', (e) => {
        e.preventDefault();
        e.dataTransfer.dropEffect = 'move';
        if (draggedSlideIndex !== null && draggedSlideIndex !== idx) {
          item.classList.add('drop-target');
        }
      });

      item.addEventListener('dragleave', () => {
        item.classList.remove('drop-target');
      });

      item.addEventListener('drop', (e) => {
        e.preventDefault();
        item.classList.remove('drop-target');
        if (draggedSlideIndex !== null && draggedSlideIndex !== idx) {
          const fromIdx = draggedSlideIndex;
          const toIdx = idx;
          const [movedSlide] = currentDeck.slides.splice(fromIdx, 1);
          currentDeck.slides.splice(toIdx, 0, movedSlide);
          currentDeck.slides.forEach((s, i) => { s.page_index = i + 1; });
          currentSlideIdx = toIdx;
          renderSlide();
          renderPageOverview();
        }
      });

      item.addEventListener('click', () => {
        if (!item.classList.contains('dragging')) {
          currentSlideIdx = idx;
          renderSlide();
          if (pageOverviewOverlay) pageOverviewOverlay.style.display = 'none';
          toolBtns.thumbnails.classList.remove('active');
          const bar = document.getElementById('page-action-bar');
          if (bar) bar.style.display = 'none';
        }
      });

      overviewScrollContainer.appendChild(item);
    });
  }

  Object.keys(toolBtns).forEach(k => {
    if (k === 'thumbnails') {
      toolBtns[k].addEventListener('click', () => {
        const bar = document.getElementById('page-action-bar');
        const isActive = toolBtns.thumbnails.classList.toggle('active');
        if (bar) bar.style.display = isActive ? 'flex' : 'none';
        if (pageOverviewOverlay) {
          pageOverviewOverlay.style.display = isActive ? 'flex' : 'none';
          if (isActive) renderPageOverview();
        }
        updateToolIcons();
      });
    } else {
      toolBtns[k].addEventListener('click', () => selectTool(k));
    }
  });

  // Color Slots
  // Color Slots & Floating Color Popover
  const slotYellow = document.getElementById('slot-yellow');
  const slotGreen = document.getElementById('slot-green');
  const slotBlue = document.getElementById('slot-blue');
  const slotRed = document.getElementById('slot-red');
  const slotCyan = document.getElementById('slot-cyan');
  const slotAdd = document.getElementById('slot-add');

  const colorPopoverWindow = document.getElementById('color-popover-window');
  const colorPopoverBackdrop = document.getElementById('color-popover-backdrop');
  const colorPopoverBeak = document.getElementById('color-popover-beak');
  const btnColorDone = document.getElementById('btn-color-done');
  const colorGridContainer = document.getElementById('color-grid-container');
  const paletteSwatchesRow = document.getElementById('palette-swatches-row');

  let activeSlotBtn = slotYellow;
  let customPalette = ['#fef04e', '#5da638', '#5e35f7', '#ef4444', '#00c5ff'];

  const SPECTRUM_COLORS = [
    // Row 0: Greyscale
    '#ffffff', '#e5e5ea', '#d1d1d6', '#c7c7cc', '#aeaeb2', '#8e8e93', '#636366', '#48484a', '#3a3a3c', '#2c2c2e', '#1c1c1e', '#000000',
    // Row 1: Primary vivid
    '#0038ff', '#007aff', '#5856d6', '#af52de', '#ff2d55', '#ff3b30', '#ff9500', '#ffcc00', '#ffdb4d', '#34c759', '#00c7be', '#30b0c7',
    // Row 2: Deep rich
    '#0021b0', '#0051a8', '#3634a3', '#7a22a3', '#b80028', '#b81408', '#b86200', '#b88c00', '#a38400', '#1b8a36', '#008781', '#187a8a',
    // Row 3: Dark tones
    '#001366', '#00336b', '#1e1c66', '#4a0e66', '#730019', '#730c05', '#733c00', '#735700', '#665200', '#0e5420', '#005450', '#0a4752',
    // Row 4: Bright pastels
    '#3361ff', '#3395ff', '#7977de', '#bf75e5', '#ff5777', '#ff6259', '#ffaa33', '#ffd633', '#ffe270', '#5cd27a', '#33d2cb', '#59c0d2',
    // Row 5: Medium pastels
    '#57c5ff', '#5cb3ff', '#8f8df0', '#ce8df0', '#ff8da5', '#ff948d', '#ffbf66', '#ffe066', '#ffe994', '#85dd9b', '#66ddd7', '#80d0dd',
    // Row 6: Soft pastels
    '#7adcff', '#85bcff', '#b08fff', '#d88fff', '#f98ff0', '#ff8fa4', '#ffa78f', '#ffc48f', '#ffdc8f', '#ffed8f', '#e4ff8f', '#abff8f',
    // Row 7: Light pastels
    '#bfeeff', '#c4deff', '#d8c7ff', '#ecc7ff', '#fcc7f7', '#ffc7d2', '#ffd3c7', '#ffe1c7', '#ffedc7', '#fff6c7', '#f1ffc7', '#d5ffc7',
    // Row 8: Pale pastels
    '#e0f7ff', '#e3efff', '#ece3ff', '#f6e3ff', '#fee3fc', '#ffe3e8', '#ffeae3', '#fff1e3', '#fff7e3', '#fffbe3', '#f8ffe3', '#ebffe3'
  ];

  function openColorPopover(targetBtn) {
    if (!colorPopoverWindow) return;
    colorPopoverWindow.style.display = 'flex';
    if (colorPopoverBackdrop) colorPopoverBackdrop.style.display = 'block';

    if (targetBtn && colorPopoverBeak) {
      const rect = targetBtn.getBoundingClientRect();
      const topOffset = Math.max(70, Math.min(window.innerHeight - 440, rect.top - 50));
      colorPopoverWindow.style.top = `${topOffset}px`;
      const beakTop = Math.max(20, Math.min(380, rect.top - topOffset + (rect.height / 2) - 7));
      colorPopoverBeak.style.top = `${beakTop}px`;
    }
    renderPaletteRow();
    highlightActiveSwatch();
  }

  function closeColorPopover() {
    if (colorPopoverWindow) colorPopoverWindow.style.display = 'none';
    if (colorPopoverBackdrop) colorPopoverBackdrop.style.display = 'none';
  }

  function renderColorGrid() {
    if (!colorGridContainer) return;
    colorGridContainer.innerHTML = '';
    SPECTRUM_COLORS.forEach(color => {
      const swatch = document.createElement('div');
      swatch.className = 'color-grid-swatch';
      swatch.style.backgroundColor = color;
      swatch.title = color;
      if (color.toLowerCase() === activeColor.toLowerCase()) swatch.classList.add('active');
      swatch.addEventListener('click', (e) => {
        e.stopPropagation();
        selectColor(color, activeSlotBtn);
        highlightActiveSwatch();
      });
      colorGridContainer.appendChild(swatch);
    });
  }

  function highlightActiveSwatch() {
    if (!colorGridContainer) return;
    const swatches = colorGridContainer.querySelectorAll('.color-grid-swatch');
    swatches.forEach(s => {
      const isAct = s.style.backgroundColor && (
        s.style.backgroundColor === activeColor ||
        s.title.toLowerCase() === activeColor.toLowerCase()
      );
      s.classList.toggle('active', isAct);
    });
  }

  function renderPaletteRow() {
    if (!paletteSwatchesRow) return;
    paletteSwatchesRow.innerHTML = '';
    customPalette.forEach(col => {
      const btn = document.createElement('button');
      btn.className = `palette-circle-btn ${col.toLowerCase() === activeColor.toLowerCase() ? 'active' : ''}`;
      btn.title = col;
      const dot = document.createElement('div');
      dot.className = 'palette-inner-dot';
      dot.style.background = col;
      btn.appendChild(dot);
      btn.addEventListener('click', (e) => {
        e.stopPropagation();
        selectColor(col, activeSlotBtn);
        renderPaletteRow();
        highlightActiveSwatch();
      });
      paletteSwatchesRow.appendChild(btn);
    });

    // Add (+) button
    const addBtn = document.createElement('button');
    addBtn.className = 'palette-add-btn';
    addBtn.title = 'Add Current Color to Palette';
    addBtn.textContent = '+';
    addBtn.addEventListener('click', (e) => {
      e.stopPropagation();
      if (!customPalette.includes(activeColor)) {
        customPalette.push(activeColor);
        renderPaletteRow();
      }
    });
    paletteSwatchesRow.appendChild(addBtn);
  }

  // Done button to close color selector window
  if (btnColorDone) {
    btnColorDone.addEventListener('click', (e) => {
      e.stopPropagation();
      closeColorPopover();
    });
  }

  // Click outside color selector window to close it
  if (colorPopoverBackdrop) {
    colorPopoverBackdrop.addEventListener('click', () => {
      closeColorPopover();
    });
  }

  function selectColor(color, btn) {
    activeColor = color;
    if (btn) {
      activeSlotBtn = btn;
      const circle = btn.querySelector('.color-circle');
      if (circle) circle.style.background = color;
    }
    [slotYellow, slotGreen, slotBlue, slotRed, slotCyan].forEach(b => { if (b) b.classList.remove('active'); });
    if (btn && btn !== slotAdd) btn.classList.add('active');

    // Update dynamic color bars on pen, highlighter, and laser pointer tools
    const barPen = document.getElementById('bar-pen');
    const barHighlighter = document.getElementById('bar-highlighter');
    const barLaser = document.getElementById('bar-laser');
    if (barPen) barPen.style.background = color;
    if (barHighlighter) barHighlighter.style.background = color;
    if (barLaser) barLaser.style.background = color;

    // Update presenter SVG laser dot and glow circles
    const laserDot = document.getElementById('presenter-laser-dot');
    const laserGlow = document.getElementById('presenter-laser-glow');
    if (laserDot) laserDot.setAttribute('fill', color);
    if (laserGlow) laserGlow.setAttribute('stroke', color);

    // Update canvas laser pointer dot gradient
    const stageLaserDot = document.getElementById('stage-laser-dot');
    if (stageLaserDot) {
      stageLaserDot.style.background = `radial-gradient(circle, ${color} 30%, ${color}44 70%, transparent 100%)`;
    }

    const stylePreviewCircle = document.getElementById('style-preview-circle');
    if (stylePreviewCircle) stylePreviewCircle.style.background = color;
    const wheelThumb = document.getElementById('color-wheel-thumb');
    if (wheelThumb) wheelThumb.style.background = color;
    const brightnessThumb = document.getElementById('brightness-slider-thumb');
    if (brightnessThumb) brightnessThumb.style.background = color;

    updateCanvasCursor();
    updateToolIcons();
  }

  function handleSlotClick(btn, defaultColor) {
    selectColor(defaultColor, btn);
    closeColorPopover();
  }

  function handleSlotRightClick(e, btn, defaultColor) {
    e.preventDefault();
    selectColor(defaultColor, btn);
    openColorPopover(btn);
  }

  slotYellow.addEventListener('click', () => handleSlotClick(slotYellow, slotYellow.querySelector('.color-circle').style.background || '#fef04e'));
  slotYellow.addEventListener('contextmenu', (e) => handleSlotRightClick(e, slotYellow, slotYellow.querySelector('.color-circle').style.background || '#fef04e'));

  slotGreen.addEventListener('click', () => handleSlotClick(slotGreen, slotGreen.querySelector('.color-circle').style.background || '#5da638'));
  slotGreen.addEventListener('contextmenu', (e) => handleSlotRightClick(e, slotGreen, slotGreen.querySelector('.color-circle').style.background || '#5da638'));

  slotBlue.addEventListener('click', () => handleSlotClick(slotBlue, slotBlue.querySelector('.color-circle').style.background || '#5e35f7'));
  slotBlue.addEventListener('contextmenu', (e) => handleSlotRightClick(e, slotBlue, slotBlue.querySelector('.color-circle').style.background || '#5e35f7'));

  if (slotRed) {
    slotRed.addEventListener('click', () => handleSlotClick(slotRed, slotRed.querySelector('.color-circle').style.background || '#ef4444'));
    slotRed.addEventListener('contextmenu', (e) => handleSlotRightClick(e, slotRed, slotRed.querySelector('.color-circle').style.background || '#ef4444'));
  }

  if (slotCyan) {
    slotCyan.addEventListener('click', () => handleSlotClick(slotCyan, slotCyan.querySelector('.color-circle').style.background || '#00c5ff'));
    slotCyan.addEventListener('contextmenu', (e) => handleSlotRightClick(e, slotCyan, slotCyan.querySelector('.color-circle').style.background || '#00c5ff'));
  }

  if (slotAdd) {
    slotAdd.addEventListener('click', () => {
      const isAlreadyOpen = colorPopoverWindow && colorPopoverWindow.style.display !== 'none';
      if (isAlreadyOpen) closeColorPopover();
      else openColorPopover(slotAdd);
    });
  }

  // --- COLOR WHEEL & SEGMENTED TABS LOGIC ---
  let currentHue = 60, currentSat = 1.0, currentVal = 1.0;

  function hsvToRgb(h, s, v) {
    let c = v * s;
    let hp = ((h % 360) + 360) % 360 / 60;
    let x = c * (1 - Math.abs((hp % 2) - 1));
    let r1 = 0, g1 = 0, b1 = 0;
    if (hp < 1) { r1 = c; g1 = x; }
    else if (hp < 2) { r1 = x; g1 = c; }
    else if (hp < 3) { g1 = c; b1 = x; }
    else if (hp < 4) { g1 = x; b1 = c; }
    else if (hp < 5) { r1 = x; b1 = c; }
    else { r1 = c; b1 = x; }
    let m = v - c;
    let r = Math.round((r1 + m) * 255);
    let g = Math.round((g1 + m) * 255);
    let b = Math.round((b1 + m) * 255);
    return `#${r.toString(16).padStart(2, '0')}${g.toString(16).padStart(2, '0')}${b.toString(16).padStart(2, '0')}`;
  }

  const colorTabBar = document.getElementById('color-tab-bar');
  const tabContainers = [
    document.getElementById('color-grid-container'),
    document.getElementById('color-wheel-container'),
    document.getElementById('color-thematic-container'),
    document.getElementById('color-style-container')
  ];

  if (colorTabBar) {
    colorTabBar.querySelectorAll('.color-tab-btn').forEach(btn => {
      btn.addEventListener('click', (e) => {
        e.stopPropagation();
        const tabIdx = parseInt(btn.dataset.tab, 10);
        colorTabBar.querySelectorAll('.color-tab-btn').forEach(b => b.classList.toggle('active', b === btn));
        tabContainers.forEach((cont, i) => {
          if (cont) cont.style.display = (i === tabIdx) ? (i === 0 ? 'grid' : 'flex') : 'none';
        });
      });
    });
  }

  const wheelDiscBox = document.getElementById('color-wheel-disc-box');
  const brightnessTrack = document.getElementById('brightness-slider-track');

  function updateWheelColor(e) {
    if (!wheelDiscBox) return;
    const rect = wheelDiscBox.getBoundingClientRect();
    const cx = rect.width / 2;
    const cy = rect.height / 2;
    const dx = e.clientX - rect.left - cx;
    const dy = e.clientY - rect.top - cy;
    const dist = Math.hypot(dx, dy);
    const maxR = cx - 4;
    const clampedDist = Math.min(dist, maxR);
    const angle = Math.atan2(dy, dx);
    currentHue = ((angle * 180 / Math.PI) + 360) % 360;
    currentSat = Math.max(0, Math.min(1, clampedDist / maxR));

    const thumbX = cx + Math.cos(angle) * clampedDist;
    const thumbY = cy + Math.sin(angle) * clampedDist;
    const thumbEl = document.getElementById('color-wheel-thumb');
    if (thumbEl) {
      thumbEl.style.left = `${thumbX}px`;
      thumbEl.style.top = `${thumbY}px`;
    }
    if (brightnessTrack) {
      brightnessTrack.style.background = hsvToRgb(currentHue, currentSat, 1.0);
    }
    const newHex = hsvToRgb(currentHue, currentSat, currentVal);
    selectColor(newHex, activeSlotBtn);
  }

  if (wheelDiscBox) {
    let isDraggingWheel = false;
    wheelDiscBox.addEventListener('mousedown', (e) => {
      isDraggingWheel = true;
      updateWheelColor(e);
    });
    window.addEventListener('mousemove', (e) => {
      if (isDraggingWheel) updateWheelColor(e);
    });
    window.addEventListener('mouseup', () => {
      isDraggingWheel = false;
    });
  }

  function updateBrightness(e) {
    if (!brightnessTrack) return;
    const rect = brightnessTrack.getBoundingClientRect();
    currentVal = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
    const bThumb = document.getElementById('brightness-slider-thumb');
    if (bThumb) {
      bThumb.style.left = `${currentVal * (rect.width - 16)}px`;
    }
    const newHex = hsvToRgb(currentHue, currentSat, currentVal);
    selectColor(newHex, activeSlotBtn);
  }

  if (brightnessTrack) {
    let isDraggingBrightness = false;
    brightnessTrack.addEventListener('mousedown', (e) => {
      isDraggingBrightness = true;
      updateBrightness(e);
    });
    window.addEventListener('mousemove', (e) => {
      if (isDraggingBrightness) updateBrightness(e);
    });
    window.addEventListener('mouseup', () => {
      isDraggingBrightness = false;
    });
  }

  document.querySelectorAll('.stroke-quick-btn').forEach(btn => {
    btn.addEventListener('click', (e) => {
      e.stopPropagation();
      document.querySelectorAll('.stroke-quick-btn').forEach(b => b.classList.toggle('active', b === btn));
      const w = parseFloat(btn.dataset.width);
      if (typeof currentStrokeWidth !== 'undefined') currentStrokeWidth = w;
    });
  });

  // Initialize spectrum grid
  renderColorGrid();

  // --- SLIDE NAVIGATION (ARROWS & SHORTCUTS) ---
  function goToSlide(index) {
    if (currentDeck && index >= 0 && index < currentDeck.slides.length) {
      stopMedia();
      blankMode = 0;
      currentSlideIndex = index;
      renderCurrentSlide();
    }
  }

  function goToNextSlide() {
    if (currentDeck && currentSlideIndex < currentDeck.slides.length - 1) {
      goToSlide(currentSlideIndex + 1);
    } else if (currentDeck && currentSlideIndex === currentDeck.slides.length - 1 && blankMode !== 1) {
      // Reaching end of presentation: trigger black screen
      toggleBlackout();
    }
  }

  function goToPrevSlide() {
    if (blankMode === 1) {
      toggleBlackout();
      return;
    }
    if (currentDeck && currentSlideIndex > 0) {
      goToSlide(currentSlideIndex - 1);
    }
  }

  // --- BEAMER FRAME NAVIGATION (\pause jumps) ---
  function goToNextFrame() {
    if (!currentDeck || !currentDeck.slides) return;
    const curSlide = currentDeck.slides[currentSlideIndex];
    const curFrame = curSlide.frame_index || (currentSlideIndex + 1);
    const targetIdx = currentDeck.slides.findIndex(s => (s.frame_index || 0) > curFrame);
    if (targetIdx !== -1) {
      goToSlide(targetIdx);
    } else {
      goToNextSlide();
    }
  }

  function goToPrevFrame() {
    if (!currentDeck || !currentDeck.slides) return;
    const curSlide = currentDeck.slides[currentSlideIndex];
    const curFrame = curSlide.frame_index || (currentSlideIndex + 1);
    if (curFrame <= 1) return;
    const targetFrame = curFrame - 1;
    const targetIdx = currentDeck.slides.findIndex(s => s.frame_index === targetFrame);
    if (targetIdx !== -1) {
      goToSlide(targetIdx);
    } else {
      goToPrevSlide();
    }
  }

  // --- SCREEN BLACKOUT / WHITEOUT & SCREEN SWAP ---
  function toggleBlackout() {
    blankMode = (blankMode === 1) ? 0 : 1;
    updateBlankUI();
  }

  function toggleWhiteout() {
    blankMode = (blankMode === 2) ? 0 : 2;
    updateBlankUI();
  }

  function toggleSwapScreens() {
    isScreenSwapped = !isScreenSwapped;
    renderCurrentSlide();
  }

  function updateBlankUI() {
    if (btnBlackout) btnBlackout.classList.toggle('active', blankMode === 1);
    if (btnWhiteout) btnWhiteout.classList.toggle('active', blankMode === 2);
    if (btnSwap) btnSwap.classList.toggle('active', isScreenSwapped);

    if (projectorBlankOverlay) {
      projectorBlankOverlay.className = 'projector-blank-overlay';
      if (blankMode === 1) {
        projectorBlankOverlay.classList.add('blackout');
        projectorBlankOverlay.style.display = 'block';
      } else if (blankMode === 2) {
        projectorBlankOverlay.classList.add('whiteout');
        projectorBlankOverlay.style.display = 'block';
      } else {
        projectorBlankOverlay.style.display = 'none';
      }
    }

    if (stageBlankBanner) {
      stageBlankBanner.className = 'stage-blank-banner';
      if (blankMode === 1) {
        stageBlankBanner.classList.add('blackout');
        stageBlankBanner.style.display = 'flex';
        if (stageBlankBannerText) stageBlankBannerText.textContent = '◼ AUDIENCE SCREEN BLACKOUT (B to restore)';
      } else if (blankMode === 2) {
        stageBlankBanner.classList.add('whiteout');
        stageBlankBanner.style.display = 'flex';
        if (stageBlankBannerText) stageBlankBannerText.textContent = '◻ AUDIENCE SCREEN WHITEOUT (W to restore)';
      } else if (isScreenSwapped) {
        stageBlankBanner.classList.add('swapped');
        stageBlankBanner.style.display = 'flex';
        if (stageBlankBannerText) stageBlankBannerText.textContent = '⇄ SCREENS SWAPPED (S to swap back)';
      } else {
        stageBlankBanner.style.display = 'none';
      }
    }
  }

  // --- DOCUMENT OUTLINE & TOC DRAWER ---
  function toggleTocDrawer(force) {
    isTocOpen = (force !== undefined) ? force : !isTocOpen;
    if (tocDrawerOverlay) {
      tocDrawerOverlay.style.display = isTocOpen ? 'flex' : 'none';
    }
    if (btnToc) btnToc.classList.toggle('active', isTocOpen);
    if (isTocOpen) renderTocDrawer();
  }

  function getFallbackToc() {
    if (!currentDeck || !currentDeck.slides) return [];
    const items = [];
    const seen = new Set();
    currentDeck.slides.forEach((s, idx) => {
      const fIdx = s.frame_index || (idx + 1);
      if (!seen.has(fIdx)) {
        seen.add(fIdx);
        items.push({
          title: s.title && !s.title.startsWith('Slide ') ? s.title : `Frame ${fIdx}`,
          page_index: idx + 1,
          level: 0,
          frame_index: fIdx
        });
      }
    });
    return items;
  }

  function renderTocDrawer() {
    if (!tocItemsContainer) return;
    const items = (currentDeck && currentDeck.toc && currentDeck.toc.length > 0)
      ? currentDeck.toc
      : getFallbackToc();

    tocItemsContainer.innerHTML = '';
    items.forEach(item => {
      const row = document.createElement('div');
      const isActive = (item.page_index === currentSlideIndex + 1);
      row.className = `toc-item toc-level-${item.level || 0} ${isActive ? 'active' : ''}`;

      const left = document.createElement('div');
      left.className = 'toc-item-left';

      const bullet = document.createElement('span');
      bullet.className = 'toc-bullet';
      bullet.textContent = (item.level === 0) ? '▪' : '▫';

      const label = document.createElement('span');
      label.className = 'toc-label';
      label.textContent = item.title;

      left.appendChild(bullet);
      left.appendChild(label);

      const badge = document.createElement('span');
      badge.className = 'toc-badge';
      badge.textContent = `p.${item.page_index}`;

      row.appendChild(left);
      row.appendChild(badge);

      row.addEventListener('click', () => {
        goToSlide(item.page_index - 1);
        toggleTocDrawer(false);
      });

      tocItemsContainer.appendChild(row);
    });
  }

  // Navigation (Arrow keys: Left/Right/Up/Down, Space, PageUp/PageDown, Enter)
  window.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      if (isTocOpen) {
        toggleTocDrawer(false);
        return;
      }
      closeColorPopover();
      return;
    }
    // If typing in textarea, allow Escape to blur textarea
    if (e.target.tagName === 'TEXTAREA' || e.target.tagName === 'INPUT') {
      if (e.key === 'Escape') e.target.blur();
      return;
    }

    // Right Arrow, Down Arrow, PageDown, Space, Enter -> Next Slide
    if (e.key === 'ArrowRight' || e.key === 'ArrowDown' || e.key === 'PageDown' || e.key === ' ' || e.key === 'Enter') {
      e.preventDefault();
      goToNextSlide();
    }
    // Left Arrow, Up Arrow, PageUp, Backspace -> Prev Slide
    else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp' || e.key === 'PageUp' || e.key === 'Backspace') {
      e.preventDefault();
      goToPrevSlide();
    }
    // Beamer Frame Jumping: J (Next Frame), K (Prev Frame)
    else if (e.key === 'j' || e.key === 'J') {
      e.preventDefault();
      goToNextFrame();
    }
    else if (e.key === 'k' || e.key === 'K') {
      e.preventDefault();
      goToPrevFrame();
    }
    // Screen Blanking: B (Blackout), W (Whiteout), S (Screen Swap)
    else if (e.key === 'b' || e.key === 'B') {
      e.preventDefault();
      toggleBlackout();
    }
    else if (e.key === 'w' || e.key === 'W') {
      e.preventDefault();
      toggleWhiteout();
    }
    else if (e.key === 's' || e.key === 'S') {
      e.preventDefault();
      toggleSwapScreens();
    }
    // Document Outline: O (Toggle TOC Drawer)
    else if (e.key === 'o' || e.key === 'O') {
      e.preventDefault();
      toggleTocDrawer();
    }
    else if (e.key === 'l' || e.key === 'L') {
      selectTool('laser');
    }
    else if (e.key === 'p' || e.key === 'P') {
      selectTool('pen');
    }
    else if (e.key === 'h' || e.key === 'H') {
      selectTool('highlighter');
    }
    else if (e.key === 'e' || e.key === 'E') {
      selectTool('eraser');
    }
    else if (e.key === '1') {
      document.getElementById('slot-yellow')?.click();
    }
    else if (e.key === '2') {
      document.getElementById('slot-green')?.click();
    }
    else if (e.key === '3') {
      document.getElementById('slot-blue')?.click();
    }
    else if (e.key === '4') {
      document.getElementById('slot-red')?.click();
    }
    else if (e.key === '5') {
      document.getElementById('slot-cyan')?.click();
    }
    else if (e.key === 'Tab') {
      e.preventDefault();
      document.getElementById('tool-thumbnails')?.click();
    }
  });

  // Topbar and Screen Buttons Event Listeners
  if (btnBlackout) btnBlackout.addEventListener('click', toggleBlackout);
  if (btnWhiteout) btnWhiteout.addEventListener('click', toggleWhiteout);
  if (btnSwap) btnSwap.addEventListener('click', toggleSwapScreens);
  if (btnToc) btnToc.addEventListener('click', () => toggleTocDrawer());
  if (btnCloseToc) btnCloseToc.addEventListener('click', () => toggleTocDrawer(false));
  if (tocDrawerBackdrop) tocDrawerBackdrop.addEventListener('click', () => toggleTocDrawer(false));
  if (btnPrevFrame) btnPrevFrame.addEventListener('click', goToPrevFrame);
  if (btnNextFrame) btnNextFrame.addEventListener('click', goToNextFrame);
  if (stageBlankBannerClose) {
    stageBlankBannerClose.addEventListener('click', () => {
      blankMode = 0;
      if (isScreenSwapped) isScreenSwapped = false;
      updateBlankUI();
      renderCurrentSlide();
    });
  }

  // Topbar Slide Counter Arrows
  const btnPrevSlideArrow = document.getElementById('btn-prev-slide-arrow');
  const btnNextSlideArrow = document.getElementById('btn-next-slide-arrow');
  const stageArrowPrev = document.getElementById('stage-arrow-prev');
  const stageArrowNext = document.getElementById('stage-arrow-next');

  if (btnPrevSlideArrow) {
    btnPrevSlideArrow.addEventListener('click', (e) => {
      e.stopPropagation();
      goToPrevSlide();
    });
  }

  if (btnNextSlideArrow) {
    btnNextSlideArrow.addEventListener('click', (e) => {
      e.stopPropagation();
      goToNextSlide();
    });
  }

  if (stageArrowPrev) {
    stageArrowPrev.addEventListener('click', (e) => {
      e.stopPropagation();
      goToPrevSlide();
    });
  }

  if (stageArrowNext) {
    stageArrowNext.addEventListener('click', (e) => {
      e.stopPropagation();
      goToNextSlide();
    });
  }

  // Clicking on background blurs any focused inputs so arrows work immediately
  document.addEventListener('mousedown', (e) => {
    if (e.target.tagName !== 'TEXTAREA' && e.target.tagName !== 'INPUT') {
      if (document.activeElement && (document.activeElement.tagName === 'TEXTAREA' || document.activeElement.tagName === 'INPUT')) {
        document.activeElement.blur();
      }
    }
  });

  document.getElementById('btn-back').addEventListener('click', () => {
    goToPrevSlide();
  });

  const laserCanvas = document.getElementById('laser-canvas');
  const laserCtx = laserCanvas ? laserCanvas.getContext('2d') : null;
  let isDrawingLaser = false;
  let laserTrail = []; // {x, y, time}
  let laserAnimFrame = null;

  // Canvas Drawing Logic
  function resizeCanvas() {
    const rect = stageSlideImg.getBoundingClientRect();
    if (rect.width > 0 && rect.height > 0) {
      canvas.width = rect.width;
      canvas.height = rect.height;
      if (laserCanvas) {
        laserCanvas.width = rect.width;
        laserCanvas.height = rect.height;
      }
      redrawStrokes();
    }
  }

  window.addEventListener('resize', resizeCanvas);

  function getCoords(e) {
    const rect = canvas.getBoundingClientRect();
    return {
      x: e.clientX - rect.left,
      y: e.clientY - rect.top
    };
  }

  function renderLaserTrail() {
    if (!laserCtx || !laserCanvas) return;
    const now = Date.now();
    const duration = 1800;
    laserTrail = laserTrail.filter(pt => (now - pt.time) < duration);

    laserCtx.clearRect(0, 0, laserCanvas.width, laserCanvas.height);

    if (laserTrail.length >= 2) {
      // Draw smooth quadratic spline through midpoints
      const drawPath = (ctx) => {
        ctx.beginPath();
        ctx.moveTo(laserTrail[0].x, laserTrail[0].y);
        for (let i = 1; i < laserTrail.length - 1; i++) {
          const xc = (laserTrail[i].x + laserTrail[i + 1].x) / 2;
          const yc = (laserTrail[i].y + laserTrail[i + 1].y) / 2;
          ctx.quadraticCurveTo(laserTrail[i].x, laserTrail[i].y, xc, yc);
        }
        ctx.lineTo(laserTrail[laserTrail.length - 1].x, laserTrail[laserTrail.length - 1].y);
      };

      // 1. Soft glowing outer aura
      laserCtx.save();
      laserCtx.lineCap = 'round';
      laserCtx.lineJoin = 'round';
      laserCtx.strokeStyle = activeColor + '44';
      laserCtx.lineWidth = 14;
      drawPath(laserCtx);
      laserCtx.stroke();

      // 2. Saturated beam body
      laserCtx.strokeStyle = activeColor;
      laserCtx.lineWidth = 5.5;
      drawPath(laserCtx);
      laserCtx.stroke();

      // 3. Hot white core filament
      laserCtx.strokeStyle = '#ffffff';
      laserCtx.lineWidth = 2.0;
      drawPath(laserCtx);
      laserCtx.stroke();
      laserCtx.restore();
    }

    if (laserTrail.length > 0 || isDrawingLaser) {
      laserAnimFrame = requestAnimationFrame(renderLaserTrail);
    } else {
      laserAnimFrame = null;
    }
  }

  function startLaserAnim() {
    if (!laserAnimFrame) {
      laserAnimFrame = requestAnimationFrame(renderLaserTrail);
    }
  }

  canvas.addEventListener('mousedown', (e) => {
    const pt = getCoords(e);
    if (activeTool === 'laser') {
      isDrawingLaser = true;
      laserTrail.push({ x: pt.x, y: pt.y, time: Date.now() });
      startLaserAnim();
      return;
    }

    isDrawing = true;
    currentStroke = {
      tool: activeTool,
      color: activeColor,
      points: [pt]
    };
  });

  canvas.addEventListener('mousemove', (e) => {
    const pt = getCoords(e);

    if (activeTool === 'laser') {
      const laserDot = document.getElementById('stage-laser-dot');
      laserDot.style.left = `${pt.x}px`;
      laserDot.style.top = `${pt.y}px`;

      if (isDrawingLaser) {
        laserTrail.push({ x: pt.x, y: pt.y, time: Date.now() });
        startLaserAnim();
      }
    }

    if (!isDrawing) return;
    currentStroke.points.push(pt);
    drawSegment();
  });

  window.addEventListener('mouseup', () => {
    if (isDrawingLaser) {
      isDrawingLaser = false;
    }
    if (isDrawing && currentStroke.points.length > 0) {
      isDrawing = false;
      strokes.push(currentStroke);
      currentStroke = [];
    }
  });

  function drawSegment() {
    if (currentStroke.points.length < 2) return;
    const pts = currentStroke.points;
    const p1 = pts[pts.length - 2];
    const p2 = pts[pts.length - 1];

    ctx.save();
    if (currentStroke.tool === 'highlighter') {
      ctx.strokeStyle = currentStroke.color + '77';
      ctx.lineWidth = 18;
      ctx.lineCap = 'round';
    } else if (currentStroke.tool === 'eraser') {
      ctx.globalCompositeOperation = 'destination-out';
      ctx.lineWidth = 24;
      ctx.lineCap = 'round';
    } else {
      ctx.strokeStyle = currentStroke.color;
      ctx.lineWidth = 3.5;
      ctx.lineCap = 'round';
    }

    ctx.beginPath();
    ctx.moveTo(p1.x, p1.y);
    ctx.lineTo(p2.x, p2.y);
    ctx.stroke();
    ctx.restore();
  }

  function redrawStrokes() {
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    strokes.forEach(s => {
      if (s.points.length < 2) return;
      ctx.save();
      if (s.tool === 'highlighter') {
        ctx.strokeStyle = s.color + '77';
        ctx.lineWidth = 18;
        ctx.lineCap = 'round';
      } else {
        ctx.strokeStyle = s.color;
        ctx.lineWidth = 3.5;
        ctx.lineCap = 'round';
      }
      ctx.beginPath();
      ctx.moveTo(s.points[0].x, s.points[0].y);
      for (let i = 1; i < s.points.length; i++) {
        ctx.lineTo(s.points[i].x, s.points[i].y);
      }
      ctx.stroke();
      ctx.restore();
    });
  }

  document.getElementById('tool-undo').addEventListener('click', () => {
    if (strokes.length > 0) {
      strokes.pop();
      redrawStrokes();
    }
  });

  // Timer Tick
  setInterval(() => {
    if (isTimerRunning) {
      timerSeconds++;
      const mins = String(Math.floor(timerSeconds / 60)).padStart(2, '0');
      const secs = String(timerSeconds % 60).padStart(2, '0');
      timerElapsedText.textContent = `${mins}:${secs}`;
      detachedTimerDisplay.textContent = `⏱ ${mins}:${secs}`;
    }
  }, 1000);

  // --- MODERN INTERACTIVE FLOATING TOOLTIP SYSTEM ---
  const tooltipPopup = document.createElement('div');
  tooltipPopup.className = 'presenter-tooltip-popup';
  document.body.appendChild(tooltipPopup);

  function attachTooltips() {
    document.querySelectorAll('[data-tooltip-title]').forEach(el => {
      el.addEventListener('mouseenter', () => {
        const title = el.getAttribute('data-tooltip-title') || '';
        const shortcut = el.getAttribute('data-tooltip-shortcut') || '';
        const hint = el.getAttribute('data-tooltip-hint') || '';

        let html = `<div class="presenter-tooltip-header"><span class="presenter-tooltip-title">${title}</span>`;
        if (shortcut) {
          html += `<span class="presenter-tooltip-badge">${shortcut}</span>`;
        }
        html += `</div>`;
        if (hint) {
          html += `<span class="presenter-tooltip-hint">${hint}</span>`;
        }
        tooltipPopup.innerHTML = html;

        const rect = el.getBoundingClientRect();
        // If element is in header (topbar), position below
        if (rect.top < 60) {
          tooltipPopup.style.left = `${Math.max(10, rect.left + rect.width / 2 - 80)}px`;
          tooltipPopup.style.top = `${rect.bottom + 8}px`;
        } else {
          // In sidebar or main area, position to right
          tooltipPopup.style.left = `${rect.right + 10}px`;
          tooltipPopup.style.top = `${rect.top + rect.height / 2 - 18}px`;
        }
        tooltipPopup.classList.add('visible');
      });

      el.addEventListener('mouseleave', () => {
        tooltipPopup.classList.remove('visible');
      });
      el.addEventListener('click', () => {
        tooltipPopup.classList.remove('visible');
      });
    });
  }

  // Initialize
  loadDecks();
  setSplitRatio(32);
  attachTooltips();
  selectTool('pen');
})();
