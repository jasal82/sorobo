// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { useEffect } from 'react';
import { useNavigate } from 'react-router';
import { Button, Group, Text } from '@mantine/core';
import { notifications } from '@mantine/notifications';
import { IconCloudDownload } from '@tabler/icons-react';
import { api } from '@/api/client';
import { useAction, useSettings, useUpdateStatus } from '@/api/hooks';

const NOTICE_ID = 'update-available';
/** Version already announced – the notice comes once per program start. */
let announced = '';

/**
 * Announces a new version found by the check at the start, unless the user skipped it.
 * Returns whether there is one to point at.
 */
export function useUpdateNotice(enabled: boolean): boolean {
  const navigate = useNavigate();
  const { data: u } = useUpdateStatus(enabled);
  const { data: settings } = useSettings();
  const skip = useAction((version: string) => api.post('/system/update/skip', { version }), { success: 'Sie werden an diese Version nicht mehr erinnert' }).mutate;
  const latest = u?.available ? u.latest : null;
  const current = u?.current;
  const skipped = !!latest && settings?.updates.skipped_version === latest;
  const pending = !!latest && !!settings && !skipped;

  useEffect(() => {
    if (!pending || !latest || announced === latest) return;
    announced = latest;
    notifications.show({
      id: NOTICE_ID,
      color: 'brand',
      icon: <IconCloudDownload size={18} />,
      title: `Sorobo ${latest} ist verfügbar`,
      autoClose: false,
      message: (
        <>
          <Text size="sm">Installiert ist Version {current}.</Text>
          <Group gap="xs" mt={6}>
            <Button
              size="compact-xs"
              variant="light"
              onClick={() => {
                notifications.hide(NOTICE_ID);
                navigate('/einstellungen/updates');
              }}
            >
              Ansehen
            </Button>
            <Button
              size="compact-xs"
              variant="subtle"
              color="gray"
              onClick={() => {
                notifications.hide(NOTICE_ID);
                skip(latest);
              }}
            >
              Diese Version überspringen
            </Button>
          </Group>
        </>
      ),
    });
  }, [pending, latest, current, navigate, skip]);

  return pending;
}
