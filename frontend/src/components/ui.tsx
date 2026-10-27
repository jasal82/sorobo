// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { useState, type ReactNode } from 'react';
import { Link, useBlocker } from 'react-router';
import {
  Anchor,
  Badge,
  Box,
  Button,
  Card,
  Group,
  Modal,
  Paper,
  Skeleton,
  Stack,
  Text,
  ThemeIcon,
  Title,
  EmptyState,
  type MantineColor,
} from '@mantine/core';
import { modals } from '@mantine/modals';
import { IconArrowDownRight, IconArrowLeft, IconArrowUpRight } from '@tabler/icons-react';
import { useLeaveWarning } from '@/lib/desktop';
import { money } from '@/lib/format';
import { statusInfo } from '@/lib/labels';

export function Page({ children, size = 1400 }: { children: ReactNode; size?: number | 'full' }) {
  return (
    <Box px={{ base: 'md', sm: 'xl' }} py="lg" mx="auto" maw={size === 'full' ? undefined : size}>
      {children}
    </Box>
  );
}

export function PageHeader({
  title,
  subtitle,
  actions,
  back,
  badge,
}: {
  title: ReactNode;
  subtitle?: ReactNode;
  actions?: ReactNode;
  back?: { to: string; label: string };
  badge?: ReactNode;
}) {
  return (
    <Stack gap={6} mb="lg">
      {back && (
        <Anchor component={Link} to={back.to} size="sm" c="dimmed" w="fit-content">
          <Group gap={4}>
            <IconArrowLeft size={14} />
            {back.label}
          </Group>
        </Anchor>
      )}
      <Group justify="space-between" align="flex-end" gap="md" wrap="wrap">
        <Box style={{ minWidth: 0 }}>
          <Group gap="sm" align="center">
            <Title order={1}>{title}</Title>
            {badge}
          </Group>
          {subtitle && (
            <Text c="dimmed" size="sm" mt={4}>
              {subtitle}
            </Text>
          )}
        </Box>
        {actions && (
          <Group gap="xs" wrap="wrap">
            {actions}
          </Group>
        )}
      </Group>
    </Stack>
  );
}

export function StatusBadge({ status, docType, daysOverdue = 0, size = 'md' }: { status: string; docType?: string; daysOverdue?: number; size?: 'sm' | 'md' | 'lg' }) {
  const s = statusInfo(status, docType, daysOverdue);
  return (
    <Badge color={s.color} size={size} variant="light">
      {s.label}
    </Badge>
  );
}

export function Money({ value, colored = false, fw, size, c }: { value: number; colored?: boolean; fw?: number; size?: string; c?: MantineColor }) {
  const color = c ?? (colored ? (value < 0 ? 'red.7' : value > 0 ? 'teal.7' : undefined) : undefined);
  return (
    <Text span className="tabular" fw={fw} size={size} c={color} style={{ whiteSpace: 'nowrap' }}>
      {money(value)}
    </Text>
  );
}

export function KpiCard({
  label,
  value,
  hint,
  icon,
  color = 'brand',
  trend,
  loading,
  onClick,
}: {
  label: string;
  value: ReactNode;
  hint?: ReactNode;
  icon?: ReactNode;
  color?: MantineColor;
  trend?: number | null;
  loading?: boolean;
  onClick?: () => void;
}) {
  return (
    <Card padding="lg" onClick={onClick} style={onClick ? { cursor: 'pointer' } : undefined}>
      <Group justify="space-between" align="flex-start" wrap="nowrap">
        <Stack gap={4} style={{ minWidth: 0 }}>
          <Text size="sm" c="dimmed" fw={500}>
            {label}
          </Text>
          {loading ? (
            <Skeleton h={30} w={140} />
          ) : (
            <Text fz={26} fw={700} className="tabular" lh={1.2} style={{ whiteSpace: 'nowrap' }}>
              {value}
            </Text>
          )}
          {(hint || trend !== undefined) && (
            <Group gap={6} wrap="nowrap">
              {trend !== undefined && trend !== null && Number.isFinite(trend) && (
                <Text size="xs" fw={600} component="div" className={trend >= 0 ? 'kpi-trend-up' : 'kpi-trend-down'}>
                  <Group gap={2} wrap="nowrap">
                    {trend >= 0 ? <IconArrowUpRight size={14} /> : <IconArrowDownRight size={14} />}
                    {Math.abs(trend).toFixed(0)} %
                  </Group>
                </Text>
              )}
              {hint && (
                <Text size="xs" c="dimmed" truncate>
                  {hint}
                </Text>
              )}
            </Group>
          )}
        </Stack>
        {icon && (
          <ThemeIcon size={42} radius="md" variant="light" color={color}>
            {icon}
          </ThemeIcon>
        )}
      </Group>
    </Card>
  );
}

export function SectionCard({ title, actions, children, padding = 'lg', description }: { title?: ReactNode; actions?: ReactNode; children: ReactNode; padding?: string | number; description?: ReactNode }) {
  return (
    <Card padding={padding}>
      {(title || actions) && (
        <Group justify="space-between" mb="md" wrap="nowrap" align="flex-start">
          <Box>
            {title && (
              <Text fw={650} size="md">
                {title}
              </Text>
            )}
            {description && (
              <Text size="xs" c="dimmed" mt={2}>
                {description}
              </Text>
            )}
          </Box>
          {actions}
        </Group>
      )}
      {children}
    </Card>
  );
}

export function Empty({ icon, title, description, action }: { icon: ReactNode; title: string; description?: ReactNode; action?: ReactNode }) {
  return (
    <EmptyState icon={icon} title={title} description={description} variant="light" py={48}>
      {action && <EmptyState.Actions>{action}</EmptyState.Actions>}
    </EmptyState>
  );
}

export function InfoRow({ label, children }: { label: ReactNode; children: ReactNode }) {
  return (
    <Group justify="space-between" gap="md" wrap="nowrap" py={5}>
      <Text size="sm" c="dimmed" style={{ whiteSpace: 'nowrap' }}>
        {label}
      </Text>
      <Text size="sm" ta="right" component="div" style={{ minWidth: 0 }}>
        {children}
      </Text>
    </Group>
  );
}

export function StatPill({ label, value, color }: { label: string; value: ReactNode; color?: MantineColor }) {
  return (
    <Paper withBorder px="md" py="xs" radius="md">
      <Text size="xs" c="dimmed">
        {label}
      </Text>
      <Text fw={650} className="tabular" c={color}>
        {value}
      </Text>
    </Paper>
  );
}

export function confirmAction(opts: { title: string; message: ReactNode; confirmLabel?: string; danger?: boolean; onConfirm: () => void }) {
  modals.openConfirmModal({
    title: opts.title,
    children: <Text size="sm">{opts.message}</Text>,
    labels: { confirm: opts.confirmLabel ?? 'Bestätigen', cancel: 'Abbrechen' },
    confirmProps: { color: opts.danger ? 'red' : 'brand' },
    onConfirm: opts.onConfirm,
  });
}

/**
 * Asks before leaving a page with unsaved changes – on route changes (optionally saving first)
 * and when the browser tab (or the desktop window) is closed. `allow` lists target paths that keep the
 * page mounted (e.g. switching settings tabs).
 */
export function UnsavedChangesGuard({ when, onSave, allow }: { when: boolean; onSave?: () => Promise<unknown>; allow?: (pathname: string) => boolean }) {
  const blocker = useBlocker(({ currentLocation, nextLocation }) => when && currentLocation.pathname !== nextLocation.pathname && !allow?.(nextLocation.pathname));
  const [saving, setSaving] = useState(false);
  useLeaveWarning(when);
  const saveAndLeave = async () => {
    setSaving(true);
    try {
      await onSave?.();
      blocker.proceed?.();
    } catch {
      // the save action already shows the error – stay on the page
      blocker.reset?.();
    } finally {
      setSaving(false);
    }
  };
  return (
    <Modal opened={blocker.state === 'blocked'} onClose={() => blocker.reset?.()} title={<Text fw={650}>Ungespeicherte Änderungen</Text>}>
      <Text size="sm">Sie haben Änderungen, die noch nicht gespeichert wurden. Möchten Sie die Seite trotzdem verlassen?</Text>
      <Group justify="flex-end" mt="lg">
        <Button variant="default" onClick={() => blocker.reset?.()}>
          Weiter bearbeiten
        </Button>
        <Button color="red" variant="light" onClick={() => blocker.proceed?.()}>
          Verwerfen
        </Button>
        {onSave && (
          <Button onClick={saveAndLeave} loading={saving}>
            Speichern
          </Button>
        )}
      </Group>
    </Modal>
  );
}

/** App mark – the same "S" as the program icon and favicon. */
export function SoroboLogo({ size = 32 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 64 64" role="img" aria-label="Sorobo" style={{ flexShrink: 0, display: 'block' }}>
      <rect width="64" height="64" rx="14" fill="#2552eb" />
      <path
        d="M42.5 23.2C40.6 20.9 37 19.6 32.4 19.6C26.6 19.6 22.8 22.3 22.8 26.2C22.8 30.4 26.6 31.6 32 32.6C37.5 33.6 41.4 34.9 41.4 39.1C41.4 43.1 37.6 45.6 32 45.6C27.2 45.6 23.4 44 21.2 41.2"
        fill="none"
        stroke="#fff"
        strokeWidth={7.6}
        strokeLinejoin="round"
      />
    </svg>
  );
}
