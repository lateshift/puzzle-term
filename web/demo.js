/** The demo is a client. The separately launched bridge is a local development tool. */
addEventListener("TrunkApplicationStarted", async () => {
  /** Historical rendering specimens own their canvas when explicitly requested. */
  if (new URLSearchParams(location.search).has("scene")) {
    document.querySelector("#puzzle-canvas").style.display = "block";
    for (const id of ["terminal", "status", "connect", "search"]) document.getElementById(id).hidden = true;
    return;
  }
  const { PuzzleTerminal } = window.wasmBindings;
  const host = document.querySelector("#terminal");
  const status = document.querySelector("#status");
  const error = document.querySelector("#puzzle-error");
  /** Never insert diagnostics as HTML, nor persist the bridge's secret URL. */
  const failed = (cause) => {
    error.hidden = false;
    error.textContent = cause.message ?? String(cause);
    status.textContent = "Disconnected or failed";
  };
  try {
    let term = await PuzzleTerminal.create(host);
    let connection;
    let search = term.search;
    let progress;
    let progressFrame;
    const query = document.querySelector("#search-query");
    const count = document.querySelector("#search-count");
    /** Progress is live Rust state; UI refreshes only while this query is incomplete. */
    const showProgress = () => {
      progressFrame = undefined;
      if (!progress) return;
      count.textContent = `${progress.current === undefined ? 0 : progress.current + 1} / ${progress.count}${progress.complete ? "" : " (searching…)"}`;
      if (!progress.complete) progressFrame = requestAnimationFrame(showProgress);
    };
    /** Replace the handle without retaining the previous terminal or polling after disposal. */
    const setProgress = (next) => {
      if (progressFrame !== undefined) cancelAnimationFrame(progressFrame);
      progress?.free();
      progress = next;
      showProgress();
    };
    const subscribe = () => {
      term.on("error", failed);
      term.on("title", (title) => { document.title = title || "Puzzle Terminal"; });
      term.on("searchRequested", () => { query.focus(); query.select(); });
    };
    subscribe();
    /** These controls belong to the page, outside the terminal's GPU canvas. */
    document.querySelector("#search").addEventListener("submit", (event) => {
      event.preventDefault();
      try {
        const options = document.querySelector("#search-case").checked ? { caseSensitive: true } : undefined;
        setProgress(search.find(query.value, options));
      } catch (cause) { failed(cause); }
    });
    document.querySelector("#search-next").addEventListener("click", () => setProgress(search.next()));
    document.querySelector("#search-prev").addEventListener("click", () => setProgress(search.prev()));
    document.querySelector("#search-clear").addEventListener("click", () => {
      search.clear(); setProgress(undefined); count.textContent = ""; term.focus();
    });
    await term.writeln("Puzzle Terminal — connect to your local development bridge below.");
    status.textContent = "Ready (no transport)";
    /** Input is sent only to the explicitly supplied transport, never a bundled server. */
    document.querySelector("#connect").addEventListener("submit", async (event) => {
      event.preventDefault();
      try {
        connection?.disconnect();
        /** A new child needs a fresh Session, not the previous child's held-exit state. */
        setProgress(undefined);
        search.free();
        term.dispose();
        term = await PuzzleTerminal.create(host);
        search = term.search;
        subscribe();
        connection = term.connect(document.querySelector("#bridge-url").value, {
          protocol: "puzzle-bridge-v1",
        });
        error.hidden = true;
        status.textContent = "Bridge connection requested; the shell appears after hello";
        term.focus();
      } catch (cause) { failed(cause); }
    });
    /** Disconnect preserves the final screen and lets the bridge reap its child. */
    document.querySelector("#disconnect").addEventListener("click", () => {
      connection?.disconnect();
      connection = undefined;
      status.textContent = "Disconnected";
    });
    /** Native fullscreen/keyboard-lock listeners belong to the mounted DOM adapter. */
    document.querySelector("#fullscreen").addEventListener("click", () => {
      host.requestFullscreen().catch(failed);
    });
    addEventListener("pagehide", () => { setProgress(undefined); search.free(); term.dispose(); }, { once: true });
  } catch (cause) { failed(cause); }
});
