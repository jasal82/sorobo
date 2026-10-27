// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { useEffect, useSyncExternalStore, type MouseEvent as ReactMouseEvent } from 'react';
import { Button, Group, Text } from '@mantine/core';
import { notifications } from '@mantine/notifications';
import { IconFolderOpen } from '@tabler/icons-react';

/**
 * Bridge to the desktop app (`sorobo-desktop`, native window around the same frontend).
 * In the browser all of this is inactive.
 */
declare global {
  interface Window {
    __SOROBO_DESKTOP__?: boolean;
    /** Asked by the desktop window before closing: number of editors with unsaved changes. */
    __soroboUnsaved?: () => number;
    /** Called by the desktop window after a download was saved. */
    __soroboDownloaded?: (path: string) => void;
    /** Set by the desktop window when the page draws the title bar (Windows). */
    __SOROBO_TITLEBAR__?: boolean;
    /** Called by the desktop window when it is maximized/restored or (de)activated. */
    __soroboWindowState?: (state: WindowState) => void;
    ipc?: { postMessage: (message: string) => void };
  }
}

export const isMac = typeof navigator !== 'undefined' && /Mac/i.test(navigator.platform || navigator.userAgent);

/** Label of a shortcut as the platform names its keys: ⌘K / ⌥N on macOS, Strg + K / Alt+N elsewhere. */
export const shortcutLabel = (key: string, modifier: 'mod' | 'alt') => {
  if (isMac) return `${modifier === 'mod' ? '⌘' : '⌥'}${key}`;
  return modifier === 'mod' ? `Strg + ${key}` : `Alt+${key}`;
};

export const isDesktop = typeof window !== 'undefined' && !!window.__SOROBO_DESKTOP__;

// ---------------------------------------------------------------- Title bar (Windows)

/** The window has no native frame: the page draws the title bar. */
export const hasTitlebar = isDesktop && !!window.__SOROBO_TITLEBAR__;

export interface WindowState {
  maximized: boolean;
  focused: boolean;
}

type Store<T> = { value: T; listeners: Set<() => void> };
const store = <T,>(value: T): Store<T> => ({ value, listeners: new Set() });
const setStore = <T,>(s: Store<T>, value: T) => {
  s.value = value;
  s.listeners.forEach((l) => l());
};
const useStore = <T,>(s: Store<T>) =>
  useSyncExternalStore(
    (l) => {
      s.listeners.add(l);
      return () => s.listeners.delete(l);
    },
    () => s.value,
  );

const windowState = store<WindowState>({ maximized: false, focused: true });
/** Number of mounted app headers that act as title bar (the fallback strip is hidden then). */
const titlebarHosts = store(0);

export const useWindowState = () => useStore(windowState);

export const windowCommand = (command: 'minimize' | 'maximize' | 'close' | 'drag' | 'state' | `resize-${string}`) => window.ipc?.postMessage(`window:${command}`);

/** Marks the calling component (the app header) as title bar while it is mounted. */
export function useTitlebarHost() {
  useEffect(() => {
    if (!hasTitlebar) return;
    setStore(titlebarHosts, titlebarHosts.value + 1);
    return () => setStore(titlebarHosts, titlebarHosts.value - 1);
  }, []);
}

const INTERACTIVE = 'button, a, input, textarea, select, label, [role="button"], [role="combobox"], [role="menuitem"], [data-no-drag]';
let lastDown = { time: 0, x: 0, y: 0 };

/**
 * `onMouseDown` of a title bar: moves the window when an empty spot is dragged, maximizes or
 * restores it on a double click. Buttons, inputs and links inside keep working.
 */
export function titlebarMouseDown(e: ReactMouseEvent) {
  if (!hasTitlebar || e.button !== 0 || (e.target as HTMLElement).closest(INTERACTIVE)) return;
  e.preventDefault();
  const now = Date.now();
  // The native drag loop swallows the mouse-up, so double clicks are recognised here.
  const double = now - lastDown.time < 500 && Math.abs(e.screenX - lastDown.x) < 5 && Math.abs(e.screenY - lastDown.y) < 5;
  lastDown = double ? { time: 0, x: 0, y: 0 } : { time: now, x: e.screenX, y: e.screenY };
  windowCommand(double ? 'maximize' : 'drag');
}

// Glyphs of the Windows caption buttons (Segoe Fluent Icons / Segoe MDL2 Assets).
const GLYPHS = { minimize: '\uE921', maximize: '\uE922', restore: '\uE923', close: '\uE8BB' };

/** Minimize, maximize/restore and close – styled like the Windows caption buttons. */
export function WindowControls() {
  const { maximized, focused } = useWindowState();
  if (!hasTitlebar) return null;
  return (
    <div className="window-controls" data-inactive={!focused || undefined} data-no-drag>
      <button type="button" tabIndex={-1} aria-label="Minimieren" title="Minimieren" onClick={() => windowCommand('minimize')}>
        {GLYPHS.minimize}
      </button>
      <button type="button" tabIndex={-1} aria-label={maximized ? 'Verkleinern' : 'Maximieren'} title={maximized ? 'Verkleinern' : 'Maximieren'} onClick={() => windowCommand('maximize')}>
        {maximized ? GLYPHS.restore : GLYPHS.maximize}
      </button>
      <button type="button" tabIndex={-1} className="close" aria-label="Schließen" title="Schließen" onClick={() => windowCommand('close')}>
        {GLYPHS.close}
      </button>
    </div>
  );
}

/**
 * Parts of the window frame drawn by the page: the top resize edge (left, right and bottom are
 * native) and – on pages without the app header (onboarding, errors, loading) – a transparent
 * title bar strip with the caption buttons.
 */
export function WindowChrome() {
  const { maximized } = useWindowState();
  const hosts = useStore(titlebarHosts);
  if (!hasTitlebar) return null;
  return (
    <>
      {hosts === 0 && (
        <div className="window-strip" onMouseDown={titlebarMouseDown}>
          <WindowControls />
        </div>
      )}
      {!maximized && (
        <>
          <div className="window-edge n" onMouseDown={(e) => e.button === 0 && windowCommand('resize-n')} />
          <div className="window-edge nw" onMouseDown={(e) => e.button === 0 && windowCommand('resize-nw')} />
          <div className="window-edge ne" onMouseDown={(e) => e.button === 0 && windowCommand('resize-ne')} />
        </>
      )}
    </>
  );
}

let unsaved = 0;

/**
 * Warns before the page is left with unsaved changes: when the browser tab is closed or
 * reloaded, and – in the desktop app – when the window is closed.
 */
export function useLeaveWarning(when: boolean) {
  useEffect(() => {
    if (!when) return;
    unsaved += 1;
    const warn = (e: BeforeUnloadEvent) => e.preventDefault();
    window.addEventListener('beforeunload', warn);
    return () => {
      unsaved -= 1;
      window.removeEventListener('beforeunload', warn);
    };
  }, [when]);
}

export function installDesktopBridge() {
  window.__soroboUnsaved = () => unsaved;
  if (!isDesktop) return;
  if (hasTitlebar) {
    window.__soroboWindowState = (state) => setStore(windowState, state);
    document.documentElement.dataset.titlebar = '';
    windowCommand('state');
  }
  window.__soroboDownloaded = (path) => {
    const name = path.split(/[\\/]/).pop() ?? path;
    const id = `download-${path}`;
    notifications.show({
      id,
      color: 'teal',
      title: 'Gespeichert',
      autoClose: 8000,
      message: (
        <>
          <Text size="sm" truncate title={path}>
            {name}
          </Text>
          <Group gap="xs" mt={6}>
            <Button size="compact-xs" variant="light" onClick={() => window.ipc?.postMessage(`open:${path}`)}>
              Öffnen
            </Button>
            <Button size="compact-xs" variant="subtle" leftSection={<IconFolderOpen size={12} />} onClick={() => window.ipc?.postMessage(`reveal:${path}`)}>
              Im Ordner anzeigen
            </Button>
          </Group>
        </>
      ),
    });
  };
}
