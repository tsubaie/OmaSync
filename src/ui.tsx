// Small building blocks in the style of macOS System Settings, on React Aria Components
// (keyboard, focus and screen-reader behaviour come from React Aria; the look is styles.css).

import type { ReactNode } from 'react';
import {
  Check,
  Circle,
  ExternalLink,
  RefreshCw,
  TriangleAlert,
  type LucideIcon,
} from 'lucide-react';
import { Button, Switch, Tooltip, TooltipTrigger } from 'react-aria-components';
import { progress } from './format';

/** A rounded-square icon tile, the System Settings way of marking a section or state. */
export function Tile({
  icon: Icon,
  kind = '',
  size = 22,
}: {
  icon: LucideIcon;
  kind?: string;
  size?: number;
}) {
  return (
    <span
      className={`tile ${kind}`}
      style={{ '--s': `${size}px` } as React.CSSProperties}
      aria-hidden="true"
    >
      <Icon size={Math.round(size * 0.6)} strokeWidth={2} />
    </span>
  );
}

/** A grouped form: optional title, a rounded box of rows, optional footer. */
export function Group({
  title,
  footer,
  children,
}: {
  title?: string;
  footer?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="group" aria-label={title}>
      {title && <h2>{title}</h2>}
      <div className="box">{children}</div>
      {footer && <p>{footer}</p>}
    </section>
  );
}

/** One form row: optional leading mark, a title with a detail line, an optional trailing control. */
export function Row({
  lead,
  title,
  detail,
  warn,
  end,
  children,
}: {
  lead?: ReactNode;
  title: ReactNode;
  detail?: ReactNode;
  warn?: boolean;
  end?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <div className="row">
      {lead}
      <div className="text">
        <div>{title}</div>
        {detail && <div className={`detail ${warn ? 'warn' : ''}`}>{detail}</div>}
        {children}
      </div>
      {end}
    </div>
  );
}

/** A macOS push button; at most one prominent (accent-filled) per view. */
export function Push({
  children,
  prominent,
  large,
  bad,
  disabled,
  onPress,
  tip,
}: {
  children: ReactNode;
  prominent?: boolean;
  /** The one main action on a screen, a size up. */
  large?: boolean;
  /** Takes something away: red. */
  bad?: boolean;
  disabled?: boolean;
  onPress: () => void;
  tip?: string;
}) {
  const button = (
    <Button
      className={`push ${prominent ? 'prominent' : ''} ${large ? 'large' : ''} ${bad ? 'bad' : ''}`}
      isDisabled={disabled}
      onPress={onPress}
    >
      {children}
    </Button>
  );
  return tip ? <Tip label={tip}>{button}</Tip> : button;
}

/** A text-only action, padded to a 24 px tall hit area. */
export function Link({
  children,
  disabled,
  label,
  out,
  accent,
  bad,
  expanded,
  onPress,
}: {
  children: ReactNode;
  disabled?: boolean;
  /** A fuller name for assistive tech, when the text alone repeats down a list. */
  label?: string;
  /** Opens a web page outside the app: the text ends in the outward mark. */
  out?: boolean;
  /** The one thing worth doing in its row, in the accent colour. */
  accent?: boolean;
  /** Takes something away: red when pointed at. */
  bad?: boolean;
  /** For a link that shows and hides something: whether it's showing. */
  expanded?: boolean;
  onPress: () => void;
}) {
  return (
    <Button
      className={accent ? 'link accent' : bad ? 'link bad' : 'link'}
      isDisabled={disabled}
      aria-label={label}
      aria-expanded={expanded}
      onPress={onPress}
    >
      {children}
      {out && <ExternalLink size={11} aria-hidden="true" />}
    </Button>
  );
}

/** A small switch, as macOS uses inside grouped form rows. */
export function Toggle({
  on,
  label,
  disabled,
  onChange,
}: {
  on: boolean;
  label: string;
  disabled?: boolean;
  onChange: (on: boolean) => void;
}) {
  return (
    <Switch
      className="switch"
      isSelected={on}
      isDisabled={disabled}
      onChange={onChange}
      aria-label={label}
    >
      <span className="track" aria-hidden="true" />
    </Switch>
  );
}

/** Shortcuts and explanations live in tooltips, as on macOS, not in the layout. */
export function Tip({ label, children }: { label: string; children: ReactNode }) {
  return (
    <TooltipTrigger delay={600}>
      {children}
      <Tooltip className="tip" offset={6}>
        {label}
      </Tooltip>
    </TooltipTrigger>
  );
}

/** The Omarchy mark, in the current text colour (white on the blue tile). */
export function OmarchyLogo({ size }: { size: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 1200 1200" aria-hidden="true">
      <path
        fill="currentColor"
        fillRule="evenodd"
        d="m1200 1200h-480v-80h400v-1040h-479.996v160h-400v720h720v-720h-80v-80h159.996v880h-400v160h-640v-1200h1200zm-1120-80h480v-80h-400l.004-400h-80.004zm0-560h80.004v-400h400v-80h-480.004z"
      />
    </svg>
  );
}

/**
 * The real progress of the running job: the steps it announced, ticked as they finish, the
 * current one with its upload or download percentage, and the ones still to come.
 */
export function Progress({
  steps,
  plan,
  percent,
}: {
  steps: string[];
  plan: string[];
  percent: number | null;
}) {
  if (plan.length === 0 || steps.length === 0) return null;
  const { items, share } = progress(steps, plan, percent);
  return (
    <div className="progress">
      <progress className="bar" aria-label="Progress" value={share} max={1} />
      <ol className="steps">
        {items.map((step, i) => {
          const state = i < steps.length - 1 ? 'done' : i === steps.length - 1 ? 'now' : 'next';
          return (
            <li key={`${i}${step}`} className={state}>
              {state === 'done' ? (
                <Check size={12} aria-hidden="true" />
              ) : state === 'now' ? (
                <RefreshCw size={12} aria-hidden="true" />
              ) : (
                <Circle size={12} aria-hidden="true" />
              )}
              {step}
              {state === 'now' && percent !== null && ` ${percent}%`}
            </li>
          );
        })}
      </ol>
    </div>
  );
}

/** A warning to read before doing something risky, with its icon. */
export function Caution({ children }: { children: ReactNode }) {
  return (
    <div className="caution" role="note">
      <TriangleAlert size={16} aria-hidden="true" />
      <p>{children}</p>
    </div>
  );
}
