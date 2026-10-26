import { useEffect, useState } from 'react';
import { Check, Package } from 'lucide-react';
import { api, type SetupState } from '../api';
import type { SyncState } from '../state';
import appIcon from '../assets/app-icon.png';
import { Group, Link, Progress, Push, Row, Tile, Toggle } from '../ui';
import { NoticeLine } from './NoticeLine';

/** What step 1 installs, named so nobody has to trust an unnamed "helper". */
const TOOLS: Record<string, [string, string]> = {
  chezmoi: ['chezmoi', 'Copies your settings files to and from your private sync space.'],
  gh: [
    'GitHub CLI',
    'Connects to your GitHub account to create and reach your private sync space.',
  ],
  git: ['Git', 'Keeps every version of your settings, which is what makes Undo possible.'],
};

/** Said when "I've Signed In" is pressed before GitHub has finished signing in. */
export const NOT_SIGNED_IN =
  "GitHub sign-in isn't finished yet. Complete the code prompt in your terminal and browser first.";

/** `begin`: starts syncing (a first computer) or joins, with the choices made. */
function action(
  s: SyncState,
  st: SetupState,
  begin: (onStep: (step: string) => void) => Promise<unknown>,
): [string, () => void] {
  const install = () => s.act('Installing', () => api.installHelpers(), s.probe);
  if (st.missing.length === 1)
    return [`Install ${TOOLS[st.missing[0]]?.[0] ?? st.missing[0]}`, install];
  if (st.missing.length > 1) return [`Install ${st.missing.length} Tools`, install];
  if (!st.signed_in && s.signingIn)
    return [
      "I've Signed In",
      () =>
        s.probe().then((now) => {
          if (now && !now.signed_in) s.setNotice({ good: true, warn: true, text: NOT_SIGNED_IN });
        }),
    ];
  if (!st.signed_in)
    return ['Sign In to GitHub', () => api.signIn().then(() => s.setSigningIn(true), s.fail)];
  if (st.found) {
    return ['Use My Synced Settings', () => s.act('Getting ready', begin, s.finishSetup, 'first')];
  }
  return ['Start Syncing', () => s.act('Getting ready', begin, s.finishSetup, 'first')];
}

const toggle = (list: string[], path: string, on: boolean) =>
  on ? [...list, path] : list.filter((p) => p !== path);

export function Setup({ s, st }: { s: SyncState; st: SetupState }) {
  const current = st.missing.length ? 0 : !st.signed_in ? 1 : 2;
  const [choosing, setChoosing] = useState(false);
  /** The folders picked to sync on a first computer; null until the person changes one. */
  const [picked, setPicked] = useState<string[] | null>(null);
  /** The folders a joining computer keeps its own version of. */
  const [keep, setKeep] = useState<string[]>([]);
  /** The apps and plugins switched from their default: on a first computer no app syncs and
   * every plugin does; a joining computer takes all the synced ones. */
  const [flipApps, setFlipApps] = useState<string[]>([]);
  const [flipPlugins, setFlipPlugins] = useState<string[]>([]);
  const waiting = s.signingIn && !st.signed_in;
  const { setSetup } = s;
  // While signing in happens in the terminal and browser, look every 2 seconds and whenever the
  // window comes back, quietly (no "Getting ready"), and move on as soon as it's done.
  useEffect(() => {
    if (!waiting) return;
    const look = () =>
      api.probe().then(
        (now) => now.signed_in && setSetup(now),
        () => {},
      );
    const timer = setInterval(look, 2000);
    window.addEventListener('focus', look);
    return () => {
      clearInterval(timer);
      window.removeEventListener('focus', look);
    };
  }, [waiting, setSetup]);
  const here = s.catalog.folders.filter((f) => f.exists);
  const defaults = new Set(st.folders.filter((f) => f.default).map((f) => f.path));
  const chosen = picked ?? here.filter((f) => defaults.has(f.path)).map((f) => f.path);
  const steps = [
    'Install the tools it needs',
    'Sign in to GitHub',
    st.found ? 'Bring in your synced settings' : "Save this computer's settings",
  ];
  // Lowercase mid-sentence, except the product name.
  const what = here
    .filter((f) => chosen.includes(f.path))
    .map((f) => (f.name.startsWith('Omarchy') ? f.name : f.name.toLowerCase()));
  const plan: [string, string][] = st.found
    ? [
        [
          "Backs up this computer's settings first",
          'To ~/.local/share/omarchy-sync/backups, so you can get them back.',
        ],
        [
          'Replaces them with your synced settings',
          'From the private omarchy-sync space in your GitHub account.',
        ],
        [
          'Lets you approve apps before installing',
          'Synced apps wait on the Sync pane; nothing installs on its own.',
        ],
        ['Syncs once a day from now on', 'You can turn this off any time on the Sync pane.'],
      ]
    : [
        [
          'Creates a private space called omarchy-sync',
          'In your GitHub account. Only you can see it.',
        ],
        [
          "Copies this computer's settings into it",
          what.length ? `Your ${what.join(', ')}.` : 'Nothing yet: choose what syncs below.',
        ],
        ['Changes nothing on this computer', 'It only reads your settings.'],
        ['Syncs once a day from now on', 'You can turn this off any time on the Sync pane.'],
      ];
  // Each step says why it's needed, not just what it does.
  const why = st.missing.length
    ? `Installing software needs your password, so Omarchy will ask for it once. Nothing else on your computer changes, and you can remove ${st.missing.length === 1 ? 'it' : 'them'} later like any other app.`
    : waiting
      ? 'A terminal window opened with a one-time code. Press Enter there to open GitHub in your browser, enter the code and approve. This page moves on by itself once you’re signed in.'
      : !st.signed_in
        ? 'Your settings are stored in a private space in your GitHub account that only you can see. Signing in happens in your browser.'
        : st.found
          ? "We found settings from your other computers. Here's what happens when you use them:"
          : "This is your first computer, so its settings become the starting point for the others. Here's what happens when you start:";
  const appOn = (app: string) => (st.found ? !flipApps.includes(app) : flipApps.includes(app));
  const pluginOn = (id: string) => !flipPlugins.includes(id);
  const [label, go] = action(s, st, (onStep) =>
    st.found
      ? api.join(onStep, keep, flipApps, flipPlugins)
      : api.start(
          onStep,
          picked ?? undefined,
          flipApps.length ? flipApps : undefined,
          flipPlugins.length
            ? st.plugins.filter((p) => pluginOn(p.id)).map((p) => p.id)
            : undefined,
        ),
  );

  return (
    <div className="setup">
      <div className="intro">
        <img className="app-icon" src={appIcon} alt="" width={88} height={88} />
        <h1>Keep Your Computers in Step</h1>
        <p>Your Omarchy settings, themes, plugins and apps, the same on every computer.</p>
        <p className="sub">Step {current + 1} of 3</p>
      </div>
      <Group>
        {steps.map((step, i) => (
          <div className="row" key={step}>
            <span
              className={`badge ${i < current ? 'done' : i === current ? 'now' : ''}`}
              aria-hidden="true"
            >
              {i < current ? <Check size={13} strokeWidth={3} /> : i + 1}
            </span>
            <span className={`step ${i === current ? 'now' : ''}`}>
              {step}
              {i < current && <span className="visually-hidden"> (done)</span>}
            </span>
          </div>
        ))}
      </Group>
      {s.idle && st.missing.length > 0 && (
        <Group>
          {st.missing.map((pkg) => (
            <Row
              key={pkg}
              lead={<Tile icon={Package} kind="apps" size={26} />}
              title={TOOLS[pkg]?.[0] ?? pkg}
              detail={`${TOOLS[pkg]?.[1] ?? ''} From Arch Linux's official packages.`}
            />
          ))}
        </Group>
      )}
      <p>{why}</p>
      {s.idle && st.signed_in && (
        <Group>
          {plan.map(([title, detail]) => (
            <Row key={title} title={title} detail={detail} />
          ))}
          <div className="row inline">
            <Link expanded={choosing} onPress={() => setChoosing(!choosing)}>
              {choosing ? 'Hide the Choices' : 'Choose What Syncs…'}
            </Link>
          </div>
        </Group>
      )}
      {s.idle && st.signed_in && choosing && (
        <Group
          title="What Syncs"
          footer={
            st.found
              ? 'Switched off: this computer keeps its own version, while your other computers stay in step. You can change this later.'
              : 'You can change this later on each pane.'
          }
        >
          {here.map((f) => {
            const on = st.found ? !keep.includes(f.path) : chosen.includes(f.path);
            return (
              <Row
                key={f.path}
                title={f.name}
                detail={st.found && !on ? 'Keep separate on this computer' : `~/${f.path}`}
                end={
                  <Toggle
                    on={on}
                    label={st.found ? `Sync ${f.name} on this computer` : `Sync ${f.name}`}
                    onChange={(v) =>
                      st.found
                        ? setKeep(toggle(keep, f.path, !v))
                        : setPicked(toggle(chosen, f.path, v))
                    }
                  />
                }
              />
            );
          })}
        </Group>
      )}
      {s.idle && st.signed_in && choosing && st.apps.length > 0 && (
        <Group
          title="Apps"
          footer={
            st.found
              ? "Switched off: it isn't offered to install on this computer, while your other computers keep it. You can change this later."
              : 'Switched on: your other computers offer to install it. You can change this later on Installed Apps.'
          }
        >
          {st.apps.map((app) => (
            <Row
              key={app}
              title={app}
              detail={st.found && !appOn(app) ? 'Not offered on this computer' : undefined}
              end={
                <Toggle
                  on={appOn(app)}
                  label={st.found ? `Sync ${app} on this computer` : `Sync ${app}`}
                  onChange={() => setFlipApps(toggle(flipApps, app, !flipApps.includes(app)))}
                />
              }
            />
          ))}
        </Group>
      )}
      {s.idle && st.signed_in && choosing && st.plugins.length > 0 && (
        <Group
          title="Plugins"
          footer={
            st.found
              ? "Switched off: it isn't added to this computer, while your other computers keep it. You can change this later."
              : 'Switched off: it stays on this computer only. You can change this later on Plugins.'
          }
        >
          {st.plugins.map((p) => (
            <Row
              key={p.id}
              title={p.name}
              detail={st.found && !pluginOn(p.id) ? 'Not added on this computer' : undefined}
              end={
                <Toggle
                  on={pluginOn(p.id)}
                  label={st.found ? `Sync ${p.name} on this computer` : `Sync ${p.name}`}
                  onChange={(v) => setFlipPlugins(toggle(flipPlugins, p.id, !v))}
                />
              }
            />
          ))}
        </Group>
      )}
      <div className="actions">
        {s.busy ? (
          <div className="working">
            <output className="loading">
              <span className="dot" />
              {s.busy}…
            </output>
            <Progress steps={s.steps} plan={s.plan} percent={s.percent} />
          </div>
        ) : (
          <>
            {waiting && (
              <Link onPress={() => api.signIn().catch(s.fail)}>Open the Sign-In Window Again</Link>
            )}
            <Push
              prominent
              large
              disabled={st.signed_in && !st.found && chosen.length === 0}
              onPress={go}
            >
              {label}
            </Push>
          </>
        )}
      </div>
      <NoticeLine s={s} />
    </div>
  );
}
