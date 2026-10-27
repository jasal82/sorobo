// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { useEffect, useState } from 'react';
import { Alert, Anchor, Badge, Button, Card, Group, Loader, Modal, Progress, ScrollArea, Stack, Switch, Text } from '@mantine/core';
import { IconAlertTriangle, IconCircleCheck, IconCloudDownload, IconExternalLink, IconRefresh } from '@tabler/icons-react';
import { api } from '@/api/client';
import { useAction, useUpdateStatus } from '@/api/hooks';
import type { Settings, UpdateStatus } from '@/api/types';
import { confirmAction } from '@/components/ui';
import { date, dateTime } from '@/lib/format';

type Patch = (fn: (s: Settings) => Settings) => void;

const busy = (u?: UpdateStatus) => !!u && ['downloading', 'installing', 'restarting'].includes(u.phase);

const megabytes = (bytes: number) => `${(bytes / 1024 / 1024).toLocaleString('de-DE', { maximumFractionDigits: 0 })} MB`;

/** GitHub's release notes are Markdown; headings, list items and bold text are enough here. */
function ReleaseNotes({ text }: { text: string }) {
  const lines = text.replace(/\r/g, '').split('\n').map((l) => l.trimEnd());
  return (
    <ScrollArea.Autosize mah={260}>
      <Stack gap={4}>
        {lines.map((line, i) => {
          if (!line.trim()) return null;
          const plain = line.replace(/\*\*/g, '');
          const heading = plain.match(/^#+\s+(.*)/);
          if (heading) {
            return (
              <Text key={i} size="sm" fw={650} mt={i ? 'xs' : 0}>
                {heading[1]}
              </Text>
            );
          }
          const item = plain.match(/^\s*[*-]\s+(.*)/);
          return (
            <Text key={i} size="sm" pl={item ? 'sm' : 0} style={{ overflowWrap: 'anywhere' }}>
              {item ? `• ${item[1]}` : plain}
            </Text>
          );
        })}
      </Stack>
    </ScrollArea.Autosize>
  );
}

/**
 * Follows the installation. The program ends for the update: the desktop window closes and the
 * new version opens its own; in the browser the page reloads as soon as the new program answers –
 * its status is idle again (the old one never returns to idle) or shows another version.
 */
function InstallModal({ u, down }: { u?: UpdateStatus; down: boolean }) {
  const [from, setFrom] = useState<string | null>(null);
  const [seconds, setSeconds] = useState(0);
  useEffect(() => {
    if (busy(u) && from === null) setFrom(u?.current ?? '');
    if (from !== null && u && !down && (u.phase === 'idle' || u.current !== from)) window.location.reload();
  }, [u, down, from]);
  const restarting = from !== null && (u?.phase === 'restarting' || down);
  useEffect(() => {
    if (!restarting) return;
    const timer = window.setInterval(() => setSeconds((n) => n + 1), 1000);
    return () => window.clearInterval(timer);
  }, [restarting]);
  return (
    <Modal opened={busy(u) || restarting} onClose={() => {}} withCloseButton={false} closeOnClickOutside={false} closeOnEscape={false} centered title={<Text fw={650}>Update auf Version {u?.latest}</Text>}>
      <Stack gap="sm">
        {u?.phase === 'downloading' && !down && (
          <>
            <Text size="sm">Die neue Version wird heruntergeladen und geprüft …</Text>
            <Progress value={u.progress * 100} animated striped />
            <Text size="xs" c="dimmed">
              {Math.round(u.progress * 100)} % von {megabytes(u.download_size)}
            </Text>
          </>
        )}
        {u?.phase === 'installing' && !down && (
          <Group gap="sm">
            <Loader size="sm" />
            <Text size="sm">Wird installiert …</Text>
          </Group>
        )}
        {restarting && (
          <Group gap="sm" wrap="nowrap">
            <Loader size="sm" />
            <Text size="sm">
              {seconds > 120
                ? 'Sorobo wurde beendet, meldet sich aber noch nicht zurück. Falls es nicht von selbst startet, bitte Sorobo neu starten.'
                : 'Sorobo wird neu gestartet – gleich geht es mit der neuen Version weiter.'}
            </Text>
          </Group>
        )}
      </Stack>
    </Modal>
  );
}

export function UpdatesTab({ s, patch }: { s: Settings; patch: Patch }) {
  const { data: u, error, refetch } = useUpdateStatus();
  const check = useAction(() => api.post<UpdateStatus>('/system/update/check'), { invalidate: false, onSuccess: () => refetch() });
  const install = useAction(() => api.post<UpdateStatus>('/system/update/install'), { invalidate: false, errorTitle: 'Update nicht möglich', onSuccess: () => refetch() });
  const remind = useAction(() => api.post('/system/update/skip', { version: '' }), { success: 'Sie werden wieder an diese Version erinnert' });

  const startInstall = () => {
    if (!u) return;
    const unsaved = window.__soroboUnsaved?.() ?? 0;
    confirmAction({
      title: `Sorobo ${u.latest} installieren`,
      message: (
        <>
          Vorher wird automatisch eine Datensicherung erstellt. Sorobo wird für das Update beendet und startet danach von selbst neu.
          {unsaved > 0 && (
            <Text component="span" display="block" mt="xs" c="orange.8" size="sm">
              Es gibt noch ungespeicherte Änderungen – sie gehen verloren, wenn Sie nicht vorher speichern.
            </Text>
          )}
        </>
      ),
      confirmLabel: 'Jetzt aktualisieren',
      onConfirm: () => install.mutate(),
    });
  };

  const skipped = !!u?.latest && s.updates.skipped_version === u.latest;
  return (
    <Stack gap="lg" maw={760}>
      <Card>
        <Group justify="space-between" align="flex-start" mb="sm" wrap="nowrap">
          <div>
            <Text fw={650}>Sorobo {u?.current ?? ''}</Text>
            <Text size="sm" c="dimmed">
              {u?.checked_at ? `Zuletzt geprüft: ${dateTime(u.checked_at)}` : 'Noch nicht nach Updates gesucht'}
            </Text>
          </div>
          <Button variant="default" leftSection={<IconRefresh size={16} />} onClick={() => check.mutate()} loading={check.isPending} disabled={busy(u)}>
            Jetzt prüfen
          </Button>
        </Group>
        {error && !busy(u) && (
          <Alert color="red" icon={<IconAlertTriangle size={16} />}>
            {error.message}
          </Alert>
        )}
        {u?.check_error && (
          <Alert color="orange" icon={<IconAlertTriangle size={16} />} title="Prüfung fehlgeschlagen">
            {u.check_error}
          </Alert>
        )}
        {u && !u.check_error && u.latest && !u.available && (
          <Group gap="xs">
            <IconCircleCheck size={18} color="var(--mantine-color-teal-6)" />
            <Text size="sm">Sorobo ist auf dem neuesten Stand.</Text>
          </Group>
        )}
        {u?.available && (
          <Stack gap="sm">
            <Group gap="xs">
              <Badge color="brand" variant="light" leftSection={<IconCloudDownload size={12} />}>
                Neue Version {u.latest}
              </Badge>
              {u.published_at && (
                <Text size="sm" c="dimmed">
                  veröffentlicht am {date(u.published_at.slice(0, 10))}
                </Text>
              )}
            </Group>
            {u.notes && (
              <Card withBorder padding="sm" bg="var(--app-bg)">
                <ReleaseNotes text={u.notes} />
              </Card>
            )}
            {u.phase === 'failed' && (
              <Alert color="red" icon={<IconAlertTriangle size={16} />} title="Update fehlgeschlagen">
                {u.error} Die bisherige Version ist unverändert.
              </Alert>
            )}
            <Text size="sm" c="dimmed">
              {u.can_install ? `${u.install_note} Download: ${megabytes(u.download_size)}.` : u.install_note}
            </Text>
            <Group gap="sm">
              {u.can_install && (
                <Button leftSection={<IconCloudDownload size={16} />} onClick={startInstall} loading={install.isPending} disabled={busy(u)}>
                  {u.phase === 'failed' ? 'Erneut versuchen' : 'Jetzt aktualisieren'}
                </Button>
              )}
              {u.release_url && (
                <Button component="a" href={u.release_url} target="_blank" rel="noreferrer" variant={u.can_install ? 'default' : 'filled'} leftSection={<IconExternalLink size={16} />}>
                  {u.can_install ? 'Auf GitHub ansehen' : 'Zum Download auf GitHub'}
                </Button>
              )}
            </Group>
            {skipped && (
              <Text size="xs" c="dimmed">
                Die Erinnerung an diese Version ist ausgeschaltet.{' '}
                <Anchor size="xs" component="button" onClick={() => remind.mutate()}>
                  Wieder erinnern
                </Anchor>
              </Text>
            )}
          </Stack>
        )}
      </Card>
      <Card>
        <Switch
          label="Beim Start automatisch nach Updates suchen"
          description="Sorobo fragt beim Start und danach täglich bei GitHub nach der neuesten Version und meldet sich, wenn es eine gibt. Dabei werden keine Daten aus Ihrer Buchhaltung übertragen."
          checked={s.updates.auto_check}
          onChange={(e) => {
            const on = e.currentTarget.checked;
            patch((x) => ({ ...x, updates: { ...x.updates, auto_check: on } }));
          }}
        />
      </Card>
      <InstallModal u={u} down={!!error} />
    </Stack>
  );
}
