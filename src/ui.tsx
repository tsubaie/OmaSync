// Small building blocks in the style of macOS System Settings, on React Aria Components
// (keyboard, focus and screen-reader behaviour come from React Aria; the look is styles.css).

import type { ReactNode } from 'react';
import type { LucideIcon } from 'lucide-react';
import { Button, Switch, Tooltip, TooltipTrigger } from 'react-aria-components';

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
  disabled,
  onPress,
  tip,
}: {
  children: ReactNode;
  prominent?: boolean;
  disabled?: boolean;
  onPress: () => void;
  tip?: string;
}) {
  const button = (
    <Button
      className={`push ${prominent ? 'prominent' : ''}`}
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
  onPress,
}: {
  children: ReactNode;
  disabled?: boolean;
  onPress: () => void;
}) {
  return (
    <Button className="link" isDisabled={disabled} onPress={onPress}>
      {children}
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
