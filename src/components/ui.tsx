/**
 * The shell's shared surfaces.
 *
 * `DESIGN.md` (repository root) is the token document these components render. Every colour, radius, shadow
 * and type step used here is a CSS custom property defined ONCE in `styles.css`; no hex value appears in this
 * file, and none should appear in any component that uses it.
 *
 * There are deliberately only a handful of pieces, because the point of this sample is that STRUCTURE does
 * the work rather than a component library (this repository adds no dependencies):
 *
 *  * `PageHeader` — one shape for a page: serif display title, a description line, the page's own actions.
 *  * `Card` — one shape for a block of content, so a page is a single scrollable column of cards rather
 *    than a wall of raw form controls.
 *  * `Field` — label above the control, guidance beneath it. The old panels interleaved labels, controls
 *    and hints in one wrapping flex row, which is the single most common readability defect they had.
 *  * `StateBlock` — ONE treatment for empty, loading, error, success, warning and info. This is the part
 *    the Halo console does best and this application did worst: an error used to be a line of text at the
 *    top of a panel, or a `title` tooltip on a badge, neither of which is where the user is looking.
 *  * `Chip` — a small status token for the top bar (save state, backup state, index issues).
 *  * `Icon` / `SpikeMark` — inline SVG only. No icon package is installed, and `DESIGN.md`'s
 *    radial-spike mark is described there as an inline SVG brand glyph rather than a token.
 */

import type { ReactNode } from 'react';
import { Feedback } from './Notifications';

// ---------------------------------------------------------------------------------------------------
// Icons: geometry only, stroked with `currentColor` so a colour can never be baked into one.
// ---------------------------------------------------------------------------------------------------

const ICONS = {
  edit: (
    <>
      <path d="M12 20h8" />
      <path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z" />
    </>
  ),
  logs: (
    <>
      <path d="M4 6h16" />
      <path d="M4 12h16" />
      <path d="M4 18h10" />
    </>
  ),
  clip: (
    <path d="M20 11l-8.5 8.5a5 5 0 0 1-7-7L13 4a3.5 3.5 0 0 1 5 5l-8.5 8.5a2 2 0 0 1-3-3L15 6" />
  ),
  image: (
    <>
      <rect x="3" y="4" width="18" height="16" rx="2" />
      <circle cx="9" cy="10" r="2" />
      <path d="M21 16l-5-5-6 6-2-2-5 5" />
    </>
  ),
  grid: (
    <>
      <rect x="3" y="3" width="7" height="7" rx="1.5" />
      <rect x="14" y="3" width="7" height="7" rx="1.5" />
      <rect x="3" y="14" width="7" height="7" rx="1.5" />
      <rect x="14" y="14" width="7" height="7" rx="1.5" />
    </>
  ),
  tag: (
    <>
      <path d="M20.6 13.4l-7.2 7.2a2 2 0 0 1-2.8 0L3 13V3h10l7.6 7.6a2 2 0 0 1 0 2.8z" />
      <circle cx="7.5" cy="7.5" r="1.5" />
    </>
  ),
  deploy: (
    <>
      <path d="M12 17V4" />
      <path d="M7 9l5-5 5 5" />
      <path d="M4 20h16" />
    </>
  ),
  archive: (
    <>
      <rect x="3" y="4" width="18" height="5" rx="1.5" />
      <path d="M5 9v10a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1V9" />
      <path d="M10 14h4" />
    </>
  ),
  sparkle: <path d="M12 3l1.8 5.2L19 10l-5.2 1.8L12 17l-1.8-5.2L5 10l5.2-1.8z" />,
  menu: (
    <>
      <path d="M4 7h16M4 12h16M4 17h16" />
      <circle cx="8" cy="7" r="1" />
      <circle cx="15" cy="12" r="1" />
      <circle cx="10" cy="17" r="1" />
    </>
  ),
  sliders: (
    <>
      <path d="M4 8h16" />
      <path d="M4 16h16" />
      <circle cx="9" cy="8" r="2" />
      <circle cx="15" cy="16" r="2" />
    </>
  ),
  settings: (
    <>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1-1.7 2.9-.2-.1a1.7 1.7 0 0 0-1.8 0l-.2.1h-3.4l-.1-.2a1.7 1.7 0 0 0-1.5-1h-.2a1.7 1.7 0 0 0-1.5 1l-.1.2H5.7L4 17l.1-.1a1.7 1.7 0 0 0 .3-1.9l-.1-.2v-3.4l.2-.1a1.7 1.7 0 0 0 1-1.5v-.2a1.7 1.7 0 0 0-1-1.5l-.2-.1V4.6l2.9-1.7.1.1a1.7 1.7 0 0 0 1.9.3l.2-.1h3.4l.1.2a1.7 1.7 0 0 0 1.5 1h.2a1.7 1.7 0 0 0 1.5-1l.1-.2h3.4L21 6l-.1.1a1.7 1.7 0 0 0-.3 1.9l.1.2v3.4l-.2.1a1.7 1.7 0 0 0-1 1.5v.2a1.7 1.7 0 0 0 1 1.5l.2.1" />
    </>
  ),
  collapse: <path d="m15 18-6-6 6-6" />,
  expand: <path d="m9 18 6-6-6-6" />,
} satisfies Record<string, ReactNode>;

export type IconName = keyof typeof ICONS;

export function Icon({ name }: { name: IconName }) {
  return (
    <svg
      className="icon"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.6}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      {ICONS[name]}
    </svg>
  );
}

/** The Anthropic radial-spike mark, as `DESIGN.md`'s "Decorative Depth" section describes it. */
export function SpikeMark({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" aria-hidden="true" focusable="false">
      <path
        d="M12 3v18M4.2 7.5l15.6 9M19.8 7.5l-15.6 9"
        stroke="currentColor"
        strokeWidth={2.2}
        strokeLinecap="round"
      />
    </svg>
  );
}

// ---------------------------------------------------------------------------------------------------
// Page header
// ---------------------------------------------------------------------------------------------------

export function PageHeader({
  title,
  description,
  actions,
  level = 1,
}: {
  /** Usually a string. The editor passes the read-only title input so its copy behaviour is unchanged. */
  title: ReactNode;
  description?: ReactNode;
  actions?: ReactNode;
  /** Embedded settings panels are sections of the page, not a second page heading. */
  level?: 1 | 2;
}) {
  const Heading = level === 1 ? 'h1' : 'h2';
  return (
    <header className={`page-header${level === 2 ? ' page-header--section' : ''}`}>
      <div className="page-header-text">
        <Heading className="page-title">{title}</Heading>
        {description !== undefined && <p className="page-lede">{description}</p>}
      </div>
      {actions && <div className="page-header-actions">{actions}</div>}
    </header>
  );
}

// ---------------------------------------------------------------------------------------------------
// Card
// ---------------------------------------------------------------------------------------------------

export function Card({
  title,
  description,
  actions,
  children,
  className = '',
  id,
}: {
  title?: ReactNode;
  description?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
  className?: string;
  id?: string;
}) {
  const head = title !== undefined || actions !== undefined;
  return (
    <section className={`card ${className}`.trim()} id={id}>
      {head && (
        <header className="card-head">
          <div className="card-head-text">
            {title !== undefined && <h2 className="card-title">{title}</h2>}
            {description !== undefined && <p className="card-desc">{description}</p>}
          </div>
          {actions && <div className="card-head-actions">{actions}</div>}
        </header>
      )}
      <div className="card-body">{children}</div>
    </section>
  );
}

// ---------------------------------------------------------------------------------------------------
// Field: label above, control, guidance beneath.
// ---------------------------------------------------------------------------------------------------

export function Field({
  label,
  hint,
  children,
  grow = false,
}: {
  label: ReactNode;
  hint?: ReactNode;
  children: ReactNode;
  /** A wider column, for a path or a URL. */
  grow?: boolean;
}) {
  return (
    <label className={`field${grow ? ' field--grow' : ''}`}>
      <span className="field-label">{label}</span>
      {children}
      {hint !== undefined && <span className="field-hint">{hint}</span>}
    </label>
  );
}

// ---------------------------------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------------------------------

export type StateTone = 'empty' | 'loading' | 'error' | 'success' | 'warning' | 'info';

/** The default ARIA role per tone. An error is announced; a settled state is only reported. */
const STATE_ROLE: Record<StateTone, 'alert' | 'status' | undefined> = {
  empty: undefined,
  loading: 'status',
  error: 'alert',
  success: 'status',
  warning: 'alert',
  info: 'status',
};

export function StateBlock({
  tone,
  title,
  description,
  actions,
  onDismiss,
  dismissLabel = '关闭',
  role,
  inline = false,
}: {
  tone: StateTone;
  /** Optional: a notice that is a single sentence reads better without a heading. */
  title?: string;
  description?: ReactNode;
  actions?: ReactNode;
  onDismiss?: () => void;
  dismissLabel?: string;
  /** Overrides the tone's default role, for the few places the old markup was explicit about it. */
  role?: 'alert' | 'status' | 'none';
  /** A block inside a card rather than a page-level statement. */
  inline?: boolean;
}) {
  if (!inline && (tone === 'success' || (onDismiss && (tone === 'error' || tone === 'warning' || tone === 'info')))) {
    return <Feedback tone={tone} title={title} message={description ?? title} actions={actions} />;
  }
  const resolved = role === 'none' ? undefined : (role ?? STATE_ROLE[tone]);
  return (
    <div className={`state state--${tone}${inline ? ' state--inline' : ''}`} role={resolved}>
      <span className="state-icon" aria-hidden="true">
        {tone === 'loading' ? <span className="spinner" /> : tone === 'empty' ? <SpikeMark /> : STATE_GLYPH[tone]}
      </span>
      <div className="state-body">
        {title !== undefined && <p className="state-title">{title}</p>}
        {description !== undefined && <div className="state-desc">{description}</div>}
        {actions && <div className="state-actions">{actions}</div>}
      </div>
      {onDismiss && (
        <button
          type="button"
          className="state-dismiss"
          onClick={onDismiss}
          aria-label={dismissLabel}
          title={dismissLabel}
        >
          {dismissLabel}
        </button>
      )}
    </div>
  );
}

const STATE_GLYPH: Record<Exclude<StateTone, 'empty' | 'loading'>, string> = {
  error: '!',
  success: '✓',
  warning: '!',
  info: 'i',
};

// ---------------------------------------------------------------------------------------------------
// Chip
// ---------------------------------------------------------------------------------------------------

export function Chip({
  tone = 'neutral',
  title,
  children,
}: {
  tone?: 'neutral' | 'success' | 'warning' | 'error' | 'primary';
  title?: string;
  children: ReactNode;
}) {
  return (
    <span className={`chip chip--${tone}`} title={title}>
      {children}
    </span>
  );
}
