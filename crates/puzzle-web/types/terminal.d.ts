/** M7 rendered terminal options. Settings are the validated session layer. */
export interface TerminalOptions {
  /** A camelCase settings patch; persistence and the complete settings API arrive in M10. */
  settings?: Record<string, unknown>;
  /** Fix both grid dimensions instead of fitting the container. */
  cols?: number;
  /** Visible rows, together with cols. */
  rows?: number;
}
/** Logical W3C identity and optional physical code, encoded in the current modes. */
export interface KeySpec {
  /** W3C KeyboardEvent.key, such as d or ArrowLeft. */
  key: string;
  /** Optional W3C KeyboardEvent.code for physical bindings and kitty base keys. */
  code?: string;
  /** Control modifier. */
  ctrl?: boolean;
  /** Alt modifier; macOS Option policy applies. */
  alt?: boolean;
  /** Shift modifier. */
  shift?: boolean;
  /** Super / Command modifier. */
  meta?: boolean;
  /** Releases and repeats are reported only when the application requests them. */
  type?: "press" | "repeat" | "release";
}
/** Browser capability query, without creating a terminal. */
export interface Support {
  /** Whether navigator.gpu exists. */
  webgpu: boolean;
  /** Reason when WebGPU is absent. */
  reason?: string;
}
/** Cell and physical dimensions handed to a host transport. */
export interface PtySize {
  /** Terminal columns. */
  cols: number;
  /** Visible rows. */
  rows: number;
  /** Text surface width in device pixels. */
  widthPx: number;
  /** Text surface height in device pixels. */
  heightPx: number;
}
/** Hosts provide the application transport; Puzzle ships no backend. */
export interface HostTransport {
  /** Encoded keys, paste and replies, in FIFO order. Void means consumed synchronously;
   * return a Promise resolved only after actual writer consumption for backpressure.
   * Do not resolve on enqueue, discard bytes, or let later writes overtake this one. */
  write(data: Uint8Array): void | Promise<void>;
  /** Optional remote size notification. */
  resize?(size: PtySize): void;
  /** Optional lifecycle cleanup. */
  close?(): void;
}
/** Call only after delivering the application's last output bytes. */
export interface ExitInfo {
  /** Unknown status is omitted or null, never a made-up success code. */
  code?: number | null;
  /** Optional signal name. */
  signal?: string;
}
/** Raw sockets omit bridge framing, resize and credit. */
export interface ConnectOptions {
  /** The local development bridge uses puzzle-bridge-v1 explicitly. */
  protocol?: "raw" | "puzzle-bridge-v1";
}
/** M7 convenience events; the full event surface is M10. */
export interface TerminalEvents {
  /** OSC title text. */
  title: string;
  /** BEL after rate limiting. */
  bell: void;
  /** Completed grid reflow. */
  resize: { cols: number; rows: number };
  /** The native textarea gained focus. */
  focus: void;
  /** The native textarea lost focus. */
  blur: void;
  /** Changed distance above the live tail, in physical pixels. */
  scroll: { offsetPx: number };
  /** Expanded inclusive range, or undefined after clearing. */
  selectionChange: Range | undefined;
  /** The search binding asks the page to open its own field. */
  searchRequested: void;
  /** A structured lifecycle or transport failure. */
  error: Error & { code: string };
}
/** Typed overload augments the generated class without broadening M7's surface. */
export interface PuzzleTerminal {
  /** Subscribe to a fact; dispose removes only this listener. Callbacks cannot veto. */
  on<E extends keyof TerminalEvents>(event: E, cb: (event: TerminalEvents[E]) => void): Disposable;
}

/** Exact retained document identity and zero-based cell column. */
export interface Position { lineId: number; col: number; }
/** Inclusive whole-cell endpoints; rectangular copy uses block. */
export interface Range { start: Position; end: Position; block?: boolean; }
/** Expansion applied by the Rust selection engine. */
export type SelectMode = "simple" | "word" | "line" | "block" | "semantic";
/** Literal search; omission enables smart case. Regex is deferred. */
export interface FindOptions { caseSensitive?: boolean; }
/** Live read-only progress of the terminal's active query, updated at refreshes. */
export interface SearchState {
  /** Surviving occurrences found so far (at most 10,000). */
  readonly count: number;
  /** Zero-based navigated occurrence, undefined before results exist. */
  readonly current?: number;
  /** Current content has been scanned or capped; new output can make this false again. */
  readonly complete: boolean;
}
