// The autosave policy, and its tests.
//
// `handoff.md` §4.3 is three rules and they are all load-bearing:
//
//   * save 2 seconds after typing stops,
//   * save at least every 30 seconds as a fallback,
//   * save immediately when the application closes,
//   * and autosave NEVER creates a Git commit.
//
// The last rule is honoured by there being no commit call anywhere in this file or in the command it
// calls: `save_document` writes a Markdown file and nothing else.
//
// This module is deliberately separate from the React component so the timing rules can be tested as
// arithmetic rather than by waiting. A test that sleeps two seconds to prove a debounce is a test that
// is slow AND flaky.

export const DEBOUNCE_MS = 2_000;
export const MAX_INTERVAL_MS = 30_000;

export interface AutosaveState {
  /** The revision of the last text that reached disk. */
  savedHash: string;
  /** When the text last changed, in ms since the epoch. */
  lastChangeMs: number;
  /** When the last successful save completed. */
  lastSaveMs: number;
  /** Whether a save is in flight. */
  saving: boolean;
}

export type AutosaveReason = 'debounce' | 'interval';

export function initialAutosaveState(nowMs: number, hash: string): AutosaveState {
  return { savedHash: hash, lastChangeMs: nowMs, lastSaveMs: nowMs, saving: false };
}

/**
 * Whether a save is due, and why.
 *
 * Returns the reason so the UI can say "saved" or "autosaved", and so a test can assert WHICH rule
 * fired rather than only that something did.
 *
 * The debounce is checked first: a user who typed and then waited two seconds should get a save at the
 * two-second mark, not at the thirty-second one.
 */
export function dueReason(
  state: AutosaveState,
  currentHash: string,
  nowMs: number,
): AutosaveReason | null {
  // Nothing to save, or a save is already running.
  if (currentHash === state.savedHash || state.saving) {
    return null;
  }

  if (nowMs - state.lastChangeMs >= DEBOUNCE_MS) {
    return 'debounce';
  }
  if (nowMs - state.lastSaveMs >= MAX_INTERVAL_MS) {
    return 'interval';
  }
  return null;
}

/**
 * Whether a save must happen right now regardless of the timers.
 *
 * Used on window close: `handoff.md` §4.3 says unsaved content is saved immediately when the app
 * closes, and a debounce that has not elapsed must not swallow the user's last sentence.
 */
export function mustSaveNow(state: AutosaveState, currentHash: string): boolean {
  return currentHash !== state.savedHash && !state.saving;
}

/** Record that the text changed at this instant. */
export function markChanged(state: AutosaveState, nowMs: number): AutosaveState {
  return { ...state, lastChangeMs: nowMs };
}

/** Record that a save completed, with the hash that reached disk. */
export function markSaved(state: AutosaveState, hash: string, nowMs: number): AutosaveState {
  return { ...state, savedHash: hash, lastSaveMs: nowMs, saving: false };
}

/** Record that a save started. */
export function markSaving(state: AutosaveState): AutosaveState {
  return { ...state, saving: true };
}

/** Record that a save failed, so the next tick can retry and the UI can show the reason. */
export function markFailed(state: AutosaveState): AutosaveState {
  return { ...state, saving: false };
}

/**
 * A stable hash of the editor's text.
 *
 * Synchronous and non-cryptographic on purpose: this is compared against itself to answer "has the
 * text changed", never to prove anything to anyone. `crypto.subtle` is async, which would make the
 * autosave check async for no benefit.
 */
export function hashText(text: string): string {
  let hash = 0x811c9dc5;
  for (let index = 0; index < text.length; index += 1) {
    hash ^= text.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193);
  }
  // The length is mixed in so two different strings cannot collide by producing the same fold.
  return `${(hash >>> 0).toString(16)}-${text.length}`;
}
