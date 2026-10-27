// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { Link, useNavigate } from 'react-router';
import {
  ActionIcon,
  Anchor,
  Badge,
  Box,
  Button,
  Card,
  Center,
  Grid,
  Group,
  Paper,
  RingProgress,
  SimpleGrid,
  Skeleton,
  Stack,
  Table,
  Text,
  ThemeIcon,
  Title,
  Tooltip,
  UnstyledButton,
} from '@mantine/core';
import { notifications } from '@mantine/notifications';
import { AreaChart, CompositeChart, DonutChart } from '@mantine/charts';
import {
  IconAlertTriangle,
  IconArrowBackUp,
  IconArrowRight,
  IconBellRinging,
  IconBuildingBank,
  IconCalculator,
  IconWorld,
  IconCash,
  IconCashBanknote,
  IconChartLine,
  IconCheck,
  IconFileInvoice,
  IconFileText,
  IconPencil,
  IconPercentage,
  IconReceipt2,
  IconWallet,
  IconCoins,
  IconTrendingUp,
  IconX,
} from '@tabler/icons-react';
import dayjs from 'dayjs';
import { api } from '@/api/client';
import { useAction, useApi } from '@/api/hooks';
import type { Dashboard, GettingStartedInput, Settings, Todo } from '@/api/types';
import { KpiCard, Page, SectionCard } from '@/components/ui';
import { date, dateTime, money, moneyCompact, moneyShort, num, relativeDays } from '@/lib/format';

const CHART_COLORS = ['brand.6', 'teal.6', 'orange.5', 'grape.5', 'cyan.6', 'gray.5'];

const TODO_ICONS: Record<string, React.ReactNode> = {
  overdue: <IconAlertTriangle size={18} />,
  dunning: <IconBellRinging size={18} />,
  transactions: <IconBuildingBank size={18} />,
  debit_returned: <IconArrowBackUp size={18} />,
  debits: <IconCashBanknote size={18} />,
  drafts: <IconPencil size={18} />,
  base_rate: <IconPercentage size={18} />,
  ustva: <IconCalculator size={18} />,
  ustva_changed: <IconCalculator size={18} />,
  zm: <IconWorld size={18} />,
  zm_changed: <IconWorld size={18} />,
};

function trend(current: number, previous: number) {
  if (!previous) return null;
  return ((current - previous) / Math.abs(previous)) * 100;
}

function TodoCard({ todo }: { todo: Todo }) {
  const navigate = useNavigate();
  return (
    <UnstyledButton onClick={() => navigate(todo.link)}>
      <Paper withBorder p="sm" radius="md" style={{ borderLeft: `3px solid var(--mantine-color-${todo.severity}-6)` }}>
        <Group gap="sm" wrap="nowrap">
          <ThemeIcon variant="light" color={todo.severity} radius="md" size={34}>
            {TODO_ICONS[todo.kind] ?? <IconAlertTriangle size={18} />}
          </ThemeIcon>
          <Box style={{ flex: 1 }}>
            <Text size="sm" fw={600}>
              {todo.count} {todo.label}
            </Text>
          </Box>
          <IconArrowRight size={16} color="var(--app-muted)" />
        </Group>
      </Paper>
    </UnstyledButton>
  );
}

function GettingStarted({ setup }: { setup: Dashboard['setup'] }) {
  const navigate = useNavigate();
  const steps = [
    { done: setup.company_complete, label: 'Firmendaten vervollständigen', hint: 'Anschrift, Steuernummer und Bankverbindung für Rechnungen', to: '/einstellungen/unternehmen' },
    { done: setup.has_logo, label: 'Logo hochladen & Layout gestalten', hint: 'Ihr Briefkopf im Layout-Designer', to: '/einstellungen/layouts' },
    { done: setup.has_contacts, label: 'Ersten Kunden anlegen', hint: 'Kontakte mit Anschrift und Zahlungsbedingungen', to: '/kontakte?neu=1' },
    { done: setup.has_articles, label: 'Produkte oder Leistungen anlegen', hint: 'Für schnelles Ausfüllen von Angeboten und Rechnungen', to: '/artikel?neu=1' },
    { done: setup.has_invoices, label: 'Erste Rechnung schreiben', hint: 'Als PDF, ZUGFeRD oder XRechnung', to: '/dokumente/neu?type=invoice' },
    { done: setup.has_receipts, label: 'Ersten Beleg erfassen', hint: 'PDF, Foto oder E-Rechnung hochladen', to: '/belege/neu' },
    { done: setup.has_bank_data, label: 'Kontoauszug importieren', hint: 'Zahlungen automatisch zuordnen', to: '/bank' },
  ];
  const hide = useAction(() => api.put<Settings>('/settings/getting-started', { hidden: true } satisfies GettingStartedInput), {
    onSuccess: () =>
      notifications.show({
        color: 'teal',
        title: '„Erste Schritte“ ausgeblendet',
        message: 'Unter Einstellungen → Übersicht können Sie die Checkliste jederzeit wieder einblenden.',
        autoClose: 6000,
      }),
  });
  const doneCount = steps.filter((s) => s.done).length;
  if (setup.hidden || (doneCount >= 5 && setup.has_invoices)) return null;
  return (
    <Card mb="lg" padding="lg">
      <Group justify="space-between" mb="md">
        <div>
          <Text fw={650}>Erste Schritte</Text>
          <Text size="sm" c="dimmed">
            {doneCount} von {steps.length} erledigt – in wenigen Minuten startklar
          </Text>
        </div>
        <Group gap="xs" wrap="nowrap" align="flex-start">
          <RingProgress size={56} thickness={6} roundCaps sections={[{ value: (doneCount / steps.length) * 100, color: 'brand' }]} label={<Text ta="center" size="xs" fw={700}>{Math.round((doneCount / steps.length) * 100)}%</Text>} />
          <Tooltip label="Erste Schritte ausblenden" withArrow>
            <ActionIcon variant="subtle" color="gray" aria-label="Erste Schritte ausblenden" onClick={() => hide.mutate()} loading={hide.isPending}>
              <IconX size={16} />
            </ActionIcon>
          </Tooltip>
        </Group>
      </Group>
      <SimpleGrid cols={{ base: 1, sm: 2, lg: 4 }} spacing="sm">
        {steps.map((s) => (
          <UnstyledButton key={s.label} onClick={() => navigate(s.to)} disabled={s.done}>
            <Paper withBorder p="sm" radius="md" h="100%" style={{ opacity: s.done ? 0.6 : 1 }}>
              <Group gap="sm" wrap="nowrap" align="flex-start">
                <ThemeIcon size={22} radius="xl" variant={s.done ? 'filled' : 'light'} color={s.done ? 'teal' : 'gray'}>
                  {s.done ? <IconCheck size={14} /> : <Text size="xs" fw={700}>{steps.indexOf(s) + 1}</Text>}
                </ThemeIcon>
                <div>
                  <Text size="sm" fw={600} td={s.done ? 'line-through' : undefined}>
                    {s.label}
                  </Text>
                  <Text size="xs" c="dimmed">
                    {s.hint}
                  </Text>
                </div>
              </Group>
            </Paper>
          </UnstyledButton>
        ))}
      </SimpleGrid>
    </Card>
  );
}

function greeting() {
  const h = new Date().getHours();
  if (h < 11) return 'Guten Morgen';
  if (h < 18) return 'Guten Tag';
  return 'Guten Abend';
}

export function DashboardPage() {
  // Future months of the running year stay empty in the chart (e.g. year-end depreciation).
  const currentMonth = new Date().getMonth() + 1;
  const navigate = useNavigate();
  const { data, isLoading } = useApi<Dashboard>('/dashboard');

  return (
    <Page>
      <Group justify="space-between" align="flex-end" mb="lg" wrap="wrap">
        <Box>
          <Text size="sm" c="dimmed">
            {dayjs().format('dddd, D. MMMM YYYY')}
          </Text>
          <Title order={1}>
            {greeting()}
            {data?.company_name ? `, ${data.company_name}` : ''}
          </Title>
        </Box>
        <Group gap="xs">
          <Button variant="default" leftSection={<IconReceipt2 size={16} />} onClick={() => navigate('/belege/neu')}>
            Beleg erfassen
          </Button>
          <Button variant="default" leftSection={<IconFileText size={16} />} onClick={() => navigate('/dokumente/neu?type=quote')}>
            Angebot
          </Button>
          <Button leftSection={<IconFileInvoice size={16} />} onClick={() => navigate('/dokumente/neu?type=invoice')}>
            Neue Rechnung
          </Button>
        </Group>
      </Group>

      {data && <GettingStarted setup={data.setup} />}

      {data && data.todos.length > 0 && (
        <SimpleGrid cols={{ base: 1, sm: 2, lg: Math.min(data.todos.length, 4) }} spacing="sm" mb="lg">
          {data.todos.map((t) => (
            <TodoCard key={t.kind} todo={t} />
          ))}
        </SimpleGrid>
      )}

      <SimpleGrid cols={{ base: 1, xs: 2, lg: 4 }} spacing="md" mb="md">
        <KpiCard
          label={`Umsatz ${data?.year ?? ''} (netto)`}
          value={data ? moneyShort(data.invoiced_year) : ''}
          trend={data ? trend(data.invoiced_year, data.invoiced_prev_year_to_date) : null}
          hint={data?.invoiced_prev_year_to_date ? 'ggü. Vorjahreszeitraum' : 'Keine Vorjahreswerte'}
          icon={<IconTrendingUp size={22} />}
          loading={isLoading}
          onClick={() => navigate('/auswertungen')}
        />
        <KpiCard
          label={`Ergebnis ${data?.year ?? ''}`}
          value={data ? moneyShort(data.profit_year) : ''}
          hint={data && data.revenue_year ? `Umsatzrendite ${num((data.profit_year / data.revenue_year) * 100, 1)} %` : undefined}
          icon={<IconChartLine size={22} />}
          color="teal"
          loading={isLoading}
          onClick={() => navigate('/berichte')}
        />
        <KpiCard
          label="Offene Forderungen"
          value={data ? moneyShort(data.open_receivables) : ''}
          hint={data ? (data.overdue_count > 0 ? `${data.overdue_count} überfällig · ${moneyShort(data.overdue_amount)}` : `${data.open_receivables_count} Rechnungen offen`) : undefined}
          icon={<IconCoins size={22} />}
          color={data && data.overdue_count > 0 ? 'red' : 'orange'}
          loading={isLoading}
          onClick={() => navigate('/offene-posten')}
        />
        <KpiCard
          label="Liquidität"
          value={data ? moneyShort(data.liquidity) : ''}
          hint="Bank und Kasse"
          icon={<IconWallet size={22} />}
          color="cyan"
          loading={isLoading}
          onClick={() => navigate('/bank')}
        />
      </SimpleGrid>
      <SimpleGrid cols={{ base: 1, xs: 2, lg: 4 }} spacing="md" mb="lg">
        <KpiCard
          label={`Umsatz ${dayjs().format('MMMM')}`}
          value={data ? moneyShort(data.invoiced_month) : ''}
          trend={data ? trend(data.invoiced_month, data.invoiced_prev_month) : null}
          hint={dayjs().date() >= dayjs().daysInMonth() ? 'ggü. Vormonat' : `ggü. Vormonat bis ${dayjs().date()}.`}
          loading={isLoading}
        />
        <KpiCard label={`Kosten ${data?.year ?? ''}`} value={data ? moneyShort(data.expense_year) : ''} hint="inkl. Abschreibungen" loading={isLoading} />
        <KpiCard
          label="Offene Verbindlichkeiten"
          value={data ? moneyShort(data.open_payables) : ''}
          hint={data ? `${data.open_payables_count} Belege unbezahlt` : undefined}
          loading={isLoading}
          onClick={() => navigate('/offene-posten?tab=verbindlichkeiten')}
        />
        <KpiCard
          label={data?.small_business ? 'Umsatzsteuer' : `USt-Zahllast ${data?.ustva_label ?? ''}`}
          value={data ? (data.small_business ? 'befreit' : money(data.ustva_zahllast)) : ''}
          hint={!data ? undefined : data.small_business ? 'Kleinunternehmerregelung' : `fällig ${date(data.ustva_due)} (${relativeDays(data.ustva_due)})`}
          loading={isLoading}
          onClick={() => navigate('/steuern/ustva')}
        />
      </SimpleGrid>

      <Grid gap="md" mb="md">
        <Grid.Col span={{ base: 12, lg: 8 }}>
          <SectionCard title="Erlöse, Kosten & Ergebnis" description={`Monatliche Entwicklung ${data?.year ?? ''} laut Buchhaltung`}>
            {data && data.months.every((m) => !m.revenue && !m.expenses) ? (
              <Center h={300}>
                <Stack align="center" gap={6}>
                  <ThemeIcon size={44} radius="xl" variant="light" color="gray">
                    <IconChartLine size={22} />
                  </ThemeIcon>
                  <Text fw={600}>Noch keine Buchungen in {data.year}</Text>
                  <Text size="sm" c="dimmed" ta="center" maw={360}>
                    Sobald Rechnungen bezahlt oder Belege gebucht sind, sehen Sie hier Ihre monatliche Entwicklung.
                  </Text>
                </Stack>
              </Center>
            ) : data ? (
              <CompositeChart
                h={300}
                data={data.months.map((m) => (m.month > currentMonth ? { label: m.label } : m))}
                dataKey="label"
                withLegend
                legendProps={{ verticalAlign: 'bottom' }}
                valueFormatter={(v) => money(v)}
                yAxisProps={{ tickFormatter: (v: number) => moneyCompact(v), width: 90 }}
                series={[
                  { name: 'revenue', label: 'Erlöse', color: 'brand.6', type: 'bar' },
                  { name: 'expenses', label: 'Kosten', color: 'orange.4', type: 'bar' },
                  { name: 'profit', label: 'Ergebnis', color: 'teal.6', type: 'line' },
                ]}
                barProps={{ radius: 4 }}
                curveType="monotone"
              />
            ) : (
              <Skeleton h={300} />
            )}
          </SectionCard>
        </Grid.Col>
        <Grid.Col span={{ base: 12, lg: 4 }}>
          <SectionCard title="Kosten nach Kategorie" description="Laufendes Jahr">
            {data ? (
              data.expense_categories.length === 0 ? (
                <Text size="sm" c="dimmed">
                  Noch keine Ausgaben gebucht.
                </Text>
              ) : (
                <Stack gap="md" align="center">
                  <DonutChart
                    size={180}
                    thickness={26}
                    withTooltip
                    tooltipDataSource="segment"
                    valueFormatter={(v) => money(v)}
                    data={data.expense_categories.map((c, i) => ({ name: c.name, value: Math.round(c.amount * 100) / 100, color: CHART_COLORS[i % CHART_COLORS.length] }))}
                  />
                  <Stack gap={6} w="100%">
                    {data.expense_categories.map((c, i) => (
                      <Group key={c.key} justify="space-between" wrap="nowrap" gap="xs">
                        <Group gap={8} wrap="nowrap" style={{ minWidth: 0 }}>
                          <Box w={10} h={10} style={{ borderRadius: 3, flexShrink: 0 }} bg={CHART_COLORS[i % CHART_COLORS.length]} />
                          <Text size="sm" truncate title={c.name}>
                            {c.name}
                          </Text>
                        </Group>
                        <Text size="sm" fw={500} className="tabular">
                          {moneyShort(c.amount)}
                        </Text>
                      </Group>
                    ))}
                  </Stack>
                </Stack>
              )
            ) : (
              <Skeleton h={300} />
            )}
          </SectionCard>
        </Grid.Col>
      </Grid>

      <Grid gap="md">
        <Grid.Col span={{ base: 12, lg: 8 }}>
          <Stack gap="md">
            <SectionCard
              title="Überfällige Rechnungen"
              actions={
                <Anchor component={Link} to="/offene-posten" size="sm">
                  Alle offenen Posten
                </Anchor>
              }
            >
              {data && data.overdue.length === 0 ? (
                <Group gap="xs">
                  <ThemeIcon variant="light" color="teal" radius="xl">
                    <IconCash size={16} />
                  </ThemeIcon>
                  <Text size="sm" c="dimmed">
                    Keine überfälligen Rechnungen – sehr gut!
                  </Text>
                </Group>
              ) : (
                <Table highlightOnHover verticalSpacing="xs">
                  <Table.Tbody>
                    {(data?.overdue ?? []).map((d) => (
                      <Table.Tr key={d.id} style={{ cursor: 'pointer' }} onClick={() => navigate(`/dokumente/${d.id}`)}>
                        <Table.Td>
                          <Text size="sm" fw={600}>
                            {d.number}
                          </Text>
                          <Text size="xs" c="dimmed">
                            {d.contact_name}
                          </Text>
                        </Table.Td>
                        <Table.Td>
                          <Badge color="red" variant="light">
                            {d.days_overdue} Tage überfällig
                          </Badge>
                        </Table.Td>
                        <Table.Td visibleFrom="sm">
                          {d.dunning_count > 0 ? (
                            <Badge color="orange" variant="dot">
                              {d.dunning_count}. Mahnstufe
                            </Badge>
                          ) : (
                            <Text size="xs" c="dimmed">
                              noch nicht gemahnt
                            </Text>
                          )}
                        </Table.Td>
                        <Table.Td ta="right">
                          <Text size="sm" fw={600} className="tabular">
                            {money(d.open_amount)}
                          </Text>
                        </Table.Td>
                      </Table.Tr>
                    ))}
                  </Table.Tbody>
                </Table>
              )}
            </SectionCard>
            <SimpleGrid cols={{ base: 1, md: 2 }} spacing="md">
              <SectionCard
                title="Offene Angebote"
                actions={
                  <Anchor component={Link} to="/angebote?status=open" size="sm">
                    Alle
                  </Anchor>
                }
              >
                <Stack gap="xs">
                  {(data?.open_quotes ?? []).length === 0 && (
                    <Text size="sm" c="dimmed">
                      Keine offenen Angebote.
                    </Text>
                  )}
                  {(data?.open_quotes ?? []).map((q) => (
                    <UnstyledButton key={q.id} onClick={() => navigate(`/dokumente/${q.id}`)}>
                      <Group justify="space-between" wrap="nowrap">
                        <Box style={{ minWidth: 0 }}>
                          <Text size="sm" fw={500} truncate>
                            {q.contact_name}
                          </Text>
                          <Text size="xs" c="dimmed">
                            {q.number} · gültig bis {date(q.valid_until)}
                          </Text>
                        </Box>
                        <Text size="sm" fw={600} className="tabular">
                          {moneyShort(q.gross_total)}
                        </Text>
                      </Group>
                    </UnstyledButton>
                  ))}
                </Stack>
              </SectionCard>
              <SectionCard
                title="Zu zahlende Belege"
                actions={
                  <Anchor component={Link} to="/offene-posten?tab=verbindlichkeiten" size="sm">
                    Alle
                  </Anchor>
                }
              >
                <Stack gap="xs">
                  {(data?.open_receipts ?? []).length === 0 && (
                    <Text size="sm" c="dimmed">
                      Alle Belege sind bezahlt.
                    </Text>
                  )}
                  {(data?.open_receipts ?? []).map((r) => (
                    <UnstyledButton key={r.receipt.id} onClick={() => navigate(`/belege/${r.receipt.id}`)}>
                      <Group justify="space-between" wrap="nowrap">
                        <Box style={{ minWidth: 0 }}>
                          <Text size="sm" fw={500} truncate>
                            {r.party}
                          </Text>
                          <Text size="xs" c={r.overdue ? 'red.7' : 'dimmed'}>
                            fällig {date(r.receipt.due_date)}
                          </Text>
                        </Box>
                        <Text size="sm" fw={600} className="tabular">
                          {money(r.open_amount)}
                        </Text>
                      </Group>
                    </UnstyledButton>
                  ))}
                </Stack>
              </SectionCard>
            </SimpleGrid>
          </Stack>
        </Grid.Col>
        <Grid.Col span={{ base: 12, lg: 4 }}>
          <Stack gap="md">
            <SectionCard title="Konten">
              <Stack gap="sm">
                {(data?.accounts ?? [])
                  .filter((a) => !a.account.archived)
                  .map((a) => (
                    <UnstyledButton key={a.account.id} onClick={() => navigate(a.account.kind === 'cash' ? '/kasse' : `/bank?konto=${a.account.id}`)}>
                      <Group justify="space-between" wrap="nowrap">
                        <Group gap="sm" wrap="nowrap">
                          <ThemeIcon variant="light" color={a.account.kind === 'cash' ? 'teal' : 'brand'} radius="md">
                            {a.account.kind === 'cash' ? <IconCash size={16} /> : <IconBuildingBank size={16} />}
                          </ThemeIcon>
                          <Box>
                            <Text size="sm" fw={500}>
                              {a.account.name}
                            </Text>
                            {a.open_count > 0 && (
                              <Text size="xs" c="blue">
                                {a.open_count} nicht zugeordnet
                              </Text>
                            )}
                          </Box>
                        </Group>
                        <Text size="sm" fw={650} className="tabular" c={a.balance < 0 ? 'red.7' : undefined}>
                          {money(a.balance)}
                        </Text>
                      </Group>
                    </UnstyledButton>
                  ))}
              </Stack>
              {data && (
                <Box mt="md">
                  <AreaChart
                    h={110}
                    data={data.months.filter((m) => m.month <= new Date().getMonth() + 1)}
                    dataKey="label"
                    series={[{ name: 'liquidity', label: 'Liquidität', color: 'cyan.6' }]}
                    curveType="monotone"
                    withXAxis={false}
                    withYAxis={false}
                    gridAxis="none"
                    valueFormatter={(v) => money(v)}
                    withDots={false}
                    fillOpacity={0.25}
                  />
                </Box>
              )}
            </SectionCard>
            <SectionCard title="Letzte Aktivitäten">
              <Stack gap={10}>
                {(data?.recent ?? []).map((a) => (
                  <Group key={a.id} gap="sm" wrap="nowrap" align="flex-start">
                    <Box w={6} h={6} mt={7} bg="brand.5" style={{ borderRadius: 3, flexShrink: 0 }} />
                    <Box>
                      <Text size="sm">{a.message}</Text>
                      <Text size="xs" c="dimmed">
                        {dateTime(a.created_at)}
                      </Text>
                    </Box>
                  </Group>
                ))}
              </Stack>
            </SectionCard>
          </Stack>
        </Grid.Col>
      </Grid>
      <Card mt="md" p={0} withBorder={false} bg="transparent" shadow="none">
        <Text size="xs" c="dimmed" ta="center">
          Alle Beträge in Euro · Umsatz = fakturierte Nettobeträge, Ergebnis = gebuchte Erlöse abzüglich Kosten
        </Text>
      </Card>
    </Page>
  );
}
