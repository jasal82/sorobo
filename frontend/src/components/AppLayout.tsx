// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { useEffect, useMemo, useState } from 'react';
import { Outlet, useLocation, useNavigate, NavLink as RouterNavLink } from 'react-router';
import {
  ActionIcon,
  AppShell,
  Badge,
  Box,
  Burger,
  Button,
  Group,
  Indicator,
  Kbd,
  Loader,
  Center,
  Menu,
  NavLink,
  ScrollArea,
  Text,
  Tooltip,
  UnstyledButton,
  useComputedColorScheme,
  useMantineColorScheme,
} from '@mantine/core';
import { useDebouncedValue, useDisclosure, useHotkeys } from '@mantine/hooks';
import { Spotlight, spotlight, type SpotlightActionData } from '@mantine/spotlight';
import {
  IconAddressBook,
  IconAlertTriangle,
  IconBook2,
  IconBuildingBank,
  IconCalculator,
  IconCash,
  IconChartBar,
  IconChartPie,
  IconChevronDown,
  IconClock,
  IconFileDownload,
  IconFileInvoice,
  IconFileText,
  IconHome2,
  IconListDetails,
  IconMoon,
  IconPackage,
  IconPlus,
  IconReceipt2,
  IconReceiptRefund,
  IconSearch,
  IconSettings,
  IconSun,
  IconTruckDelivery,
  IconUser,
  IconWritingSign,
  IconBuildingWarehouse,
  IconScale,
  IconClipboardCheck,
  IconWorld,
} from '@tabler/icons-react';
import { useApi, useStatus } from '@/api/hooks';
import { SoroboLogo } from '@/components/ui';
import { useUpdateNotice } from '@/components/UpdateNotice';
import { WindowControls, hasTitlebar, shortcutLabel, titlebarMouseDown, useTitlebarHost } from '@/lib/desktop';
import type { Dashboard, SearchHit } from '@/api/types';

interface NavItem {
  label: string;
  to: string;
  icon: React.ComponentType<{ size?: number; stroke?: number }>;
  badge?: number;
  badgeColor?: string;
  match?: string[];
}

interface NavSection {
  title?: string;
  items: NavItem[];
}

function useNavSections(counts: { overdue: number; transactions: number; drafts: number }): NavSection[] {
  return [
    { items: [{ label: 'Übersicht', to: '/', icon: IconHome2 }] },
    {
      title: 'Verkauf',
      items: [
        { label: 'Angebote', to: '/angebote', icon: IconFileText },
        { label: 'Auftragsbestätigungen', to: '/auftraege', icon: IconClipboardCheck },
        { label: 'Lieferscheine', to: '/lieferscheine', icon: IconTruckDelivery },
        { label: 'Rechnungen', to: '/rechnungen', icon: IconFileInvoice, badge: counts.drafts, badgeColor: 'gray' },
        { label: 'Gutschriften', to: '/gutschriften', icon: IconReceiptRefund },
        { label: 'Offene Posten', to: '/offene-posten', icon: IconAlertTriangle, badge: counts.overdue, badgeColor: 'red' },
        { label: 'Projektstunden', to: '/projektstunden', icon: IconClock },
      ],
    },
    {
      title: 'Einkauf & Zahlungen',
      items: [
        { label: 'Belege', to: '/belege', icon: IconReceipt2 },
        { label: 'Bank & Zahlungen', to: '/bank', icon: IconBuildingBank, badge: counts.transactions, badgeColor: 'blue' },
        { label: 'Kassenbuch', to: '/kasse', icon: IconCash },
      ],
    },
    {
      title: 'Stammdaten',
      items: [
        { label: 'Kontakte', to: '/kontakte', icon: IconAddressBook },
        { label: 'Produkte & Leistungen', to: '/artikel', icon: IconPackage },
      ],
    },
    {
      title: 'Buchhaltung',
      items: [
        { label: 'Buchungsjournal', to: '/buchhaltung/journal', icon: IconListDetails },
        { label: 'Kontenplan', to: '/buchhaltung/konten', icon: IconBook2 },
        { label: 'Anlagen & AfA', to: '/buchhaltung/anlagen', icon: IconBuildingWarehouse },
      ],
    },
    {
      title: 'Steuern & Berichte',
      items: [
        { label: 'Umsatzsteuer', to: '/steuern/ustva', icon: IconCalculator },
        { label: 'EU-Meldung (ZM)', to: '/steuern/zm', icon: IconWorld },
        { label: 'EÜR · GuV · Bilanz', to: '/berichte', icon: IconScale },
        { label: 'Auswertungen', to: '/auswertungen', icon: IconChartBar },
        { label: 'Exporte & DATEV', to: '/exporte', icon: IconFileDownload },
      ],
    },
  ];
}

function NavContent({ sections, onNavigate }: { sections: NavSection[]; onNavigate: () => void }) {
  const location = useLocation();
  const isActive = (to: string) => (to === '/' ? location.pathname === '/' : location.pathname.startsWith(to));
  return (
    <>
      {sections.map((section, i) => (
        <Box key={i}>
          {section.title && <div className="nav-section-label">{section.title}</div>}
          {section.items.map((item) => (
            <NavLink
              key={item.to}
              component={RouterNavLink}
              to={item.to}
              label={item.label}
              onClick={onNavigate}
              active={isActive(item.to)}
              leftSection={<item.icon size={18} stroke={1.7} />}
              rightSection={
                item.badge ? (
                  <Badge size="sm" variant="light" color={item.badgeColor} circle={item.badge < 10}>
                    {item.badge}
                  </Badge>
                ) : null
              }
              variant="light"
              style={{ borderRadius: 8 }}
              py={7}
            />
          ))}
        </Box>
      ))}
    </>
  );
}

function SearchSpotlight() {
  const navigate = useNavigate();
  const [query, setQuery] = useState('');
  const [debounced] = useDebouncedValue(query, 200);
  const { data: hits, isFetching } = useApi<SearchHit[]>(debounced.length >= 2 ? '/search' : null, { q: debounced });

  const staticActions: SpotlightActionData[] = useMemo(
    () => [
      { id: 'new-invoice', label: 'Neue Rechnung erstellen', group: 'Aktionen', leftSection: <IconFileInvoice size={18} />, onClick: () => navigate('/dokumente/neu?type=invoice') },
      { id: 'new-quote', label: 'Neues Angebot erstellen', group: 'Aktionen', leftSection: <IconFileText size={18} />, onClick: () => navigate('/dokumente/neu?type=quote') },
      { id: 'new-receipt', label: 'Beleg erfassen', group: 'Aktionen', leftSection: <IconReceipt2 size={18} />, onClick: () => navigate('/belege/neu') },
      { id: 'new-contact', label: 'Kontakt anlegen', group: 'Aktionen', leftSection: <IconUser size={18} />, onClick: () => navigate('/kontakte?neu=1') },
      { id: 'cash', label: 'Kassenbuchung erfassen', group: 'Aktionen', leftSection: <IconCash size={18} />, onClick: () => navigate('/kasse?neu=1') },
      { id: 'nav-dashboard', label: 'Übersicht', group: 'Navigation', leftSection: <IconHome2 size={18} />, onClick: () => navigate('/') },
      { id: 'nav-invoices', label: 'Rechnungen', group: 'Navigation', leftSection: <IconFileInvoice size={18} />, onClick: () => navigate('/rechnungen') },
      { id: 'nav-open', label: 'Offene Posten & Mahnwesen', group: 'Navigation', leftSection: <IconAlertTriangle size={18} />, onClick: () => navigate('/offene-posten') },
      { id: 'nav-receipts', label: 'Belege', group: 'Navigation', leftSection: <IconReceipt2 size={18} />, onClick: () => navigate('/belege') },
      { id: 'nav-bank', label: 'Bank & Zahlungen', group: 'Navigation', leftSection: <IconBuildingBank size={18} />, onClick: () => navigate('/bank') },
      { id: 'nav-ustva', label: 'Umsatzsteuer-Voranmeldung', group: 'Navigation', leftSection: <IconCalculator size={18} />, onClick: () => navigate('/steuern/ustva') },
      { id: 'nav-zm', label: 'Zusammenfassende Meldung', group: 'Navigation', leftSection: <IconWorld size={18} />, onClick: () => navigate('/steuern/zm') },
      { id: 'nav-euer', label: 'EÜR · GuV · Bilanz', group: 'Navigation', leftSection: <IconScale size={18} />, onClick: () => navigate('/berichte') },
      { id: 'nav-analytics', label: 'Auswertungen', group: 'Navigation', leftSection: <IconChartPie size={18} />, onClick: () => navigate('/auswertungen') },
      { id: 'nav-settings', label: 'Einstellungen', group: 'Navigation', leftSection: <IconSettings size={18} />, onClick: () => navigate('/einstellungen') },
    ],
    [navigate],
  );

  const icons: Record<string, React.ReactNode> = {
    contact: <IconUser size={18} />,
    document: <IconFileInvoice size={18} />,
    receipt: <IconReceipt2 size={18} />,
    article: <IconPackage size={18} />,
  };
  const groups: Record<string, string> = { contact: 'Kontakte', document: 'Dokumente', receipt: 'Belege', article: 'Produkte & Leistungen' };

  const actions = useMemo(() => {
    const q = query.trim().toLowerCase();
    const filtered = q ? staticActions.filter((a) => String(a.label).toLowerCase().includes(q)) : staticActions;
    const dynamic: SpotlightActionData[] = (q.length >= 2 ? hits ?? [] : []).map((h) => ({
      id: `${h.kind}-${h.id}`,
      label: h.title,
      description: h.subtitle,
      group: groups[h.kind],
      leftSection: icons[h.kind],
      onClick: () => navigate(h.link),
    }));
    return [...dynamic, ...filtered];
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query, hits, staticActions]);

  return (
    <Spotlight
      actions={actions}
      query={query}
      onQueryChange={setQuery}
      filter={(_q, a) => a}
      nothingFound={isFetching ? 'Suche …' : 'Keine Treffer'}
      highlightQuery
      limit={14}
      shortcut={['mod + K', '/']}
      searchProps={{ leftSection: <IconSearch size={18} />, placeholder: 'Suchen oder Aktion ausführen …' }}
      scrollAreaProps={{ mah: 480 }}
    />
  );
}

function ColorSchemeToggle() {
  const { setColorScheme } = useMantineColorScheme();
  const computed = useComputedColorScheme('light');
  return (
    <Tooltip label={computed === 'dark' ? 'Helles Design' : 'Dunkles Design'}>
      <ActionIcon size="lg" variant="default" onClick={() => setColorScheme(computed === 'dark' ? 'light' : 'dark')} aria-label="Farbschema wechseln">
        {computed === 'dark' ? <IconSun size={18} /> : <IconMoon size={18} />}
      </ActionIcon>
    </Tooltip>
  );
}

export function AppLayout() {
  const [opened, { toggle, close }] = useDisclosure();
  const navigate = useNavigate();
  const location = useLocation();
  const { data: status, isLoading, isFetching: statusFetching } = useStatus();
  const { data: dash } = useApi<Dashboard>(status?.onboarded ? '/dashboard' : null);
  const counts = {
    overdue: dash?.overdue_count ?? 0,
    transactions: dash?.todos.find((t) => t.kind === 'transactions')?.count ?? 0,
    drafts: dash?.todos.find((t) => t.kind === 'drafts')?.count ?? 0,
  };
  const sections = useNavSections(counts);
  const update = useUpdateNotice(!!status?.onboarded);
  useTitlebarHost();

  // Only redirect on fresh data – a stale cached status (e.g. right after onboarding) must not bounce the user back.
  useEffect(() => {
    if (status && !status.onboarded && !statusFetching) navigate('/willkommen', { replace: true });
  }, [status, statusFetching, navigate]);

  useEffect(() => {
    window.scrollTo(0, 0);
    // With the desktop title bar the main area scrolls instead of the page.
    document.querySelector('.mantine-AppShell-main')?.scrollTo(0, 0);
  }, [location.pathname]);

  useHotkeys([
    ['alt+N', () => navigate('/dokumente/neu?type=invoice'), { usePhysicalKeys: true }],
    ['alt+B', () => navigate('/belege/neu'), { usePhysicalKeys: true }],
  ]);

  if (isLoading || !status) {
    return (
      <Center h="100vh">
        <Loader />
      </Center>
    );
  }

  return (
    <AppShell
      header={{ height: 58 }}
      navbar={{ width: 264, breakpoint: 'md', collapsed: { mobile: !opened } }}
      padding={0}
      styles={{
        main: { background: 'var(--app-bg)' },
        navbar: { background: 'var(--app-surface)', borderColor: 'var(--app-border)' },
        header: { background: 'var(--app-surface)', borderColor: 'var(--app-border)' },
      }}
    >
      {/* In the desktop app on Windows the header is the title bar: drag it, double-click to maximize. */}
      <AppShell.Header pl="md" pr={hasTitlebar ? 0 : 'md'} onMouseDown={titlebarMouseDown} style={{ display: 'flex' }}>
        <Group h="100%" justify="space-between" wrap="nowrap" gap="md" style={{ flex: 1, minWidth: 0 }}>
          <Group gap="sm" wrap="nowrap" style={{ minWidth: 0, flexShrink: 1 }}>
            <Burger opened={opened} onClick={toggle} hiddenFrom="md" size="sm" />
            <UnstyledButton onClick={() => navigate('/')}>
              <Group gap={10} wrap="nowrap">
                <SoroboLogo size={32} />
                <Box visibleFrom="xs">
                  <Text fw={700} size="md" lh={1.1}>
                    Sorobo
                  </Text>
                  <Text size="xs" c="dimmed" lh={1.1} maw={200} truncate>
                    {status.company_name || 'Buchhaltung'}
                  </Text>
                </Box>
              </Group>
            </UnstyledButton>
          </Group>

          <UnstyledButton onClick={() => spotlight.open()} style={{ flex: 1, minWidth: 0, maxWidth: 460 }} visibleFrom="sm">
            <Group
              gap="xs"
              px="sm"
              h={36}
              style={{ border: '1px solid var(--app-border)', borderRadius: 8, background: 'var(--app-bg)', color: 'var(--app-muted)' }}
              wrap="nowrap"
            >
              <IconSearch size={16} />
              <Text size="sm" c="dimmed" truncate style={{ flex: 1, minWidth: 0 }}>
                Suchen nach Kunden, Rechnungen, Belegen …
              </Text>
              <Kbd size="xs" visibleFrom="md" style={{ flexShrink: 0 }}>
                {shortcutLabel('K', 'mod')}
              </Kbd>
            </Group>
          </UnstyledButton>

          <Group gap="xs" wrap="nowrap" style={{ flexShrink: 0 }}>
            <ActionIcon size="lg" variant="default" hiddenFrom="sm" onClick={() => spotlight.open()} aria-label="Suchen">
              <IconSearch size={18} />
            </ActionIcon>
            <Menu position="bottom-end" width={240}>
              <Menu.Target>
                <Button leftSection={<IconPlus size={16} />} rightSection={<IconChevronDown size={14} />}>
                  Neu
                </Button>
              </Menu.Target>
              <Menu.Dropdown>
                <Menu.Label>Verkauf</Menu.Label>
                <Menu.Item leftSection={<IconFileInvoice size={16} />} onClick={() => navigate('/dokumente/neu?type=invoice')} rightSection={<Text size="xs" c="dimmed">{shortcutLabel('N', 'alt')}</Text>}>
                  Rechnung
                </Menu.Item>
                <Menu.Item leftSection={<IconFileText size={16} />} onClick={() => navigate('/dokumente/neu?type=quote')}>
                  Angebot
                </Menu.Item>
                <Menu.Item leftSection={<IconClipboardCheck size={16} />} onClick={() => navigate('/dokumente/neu?type=order')}>
                  Auftragsbestätigung
                </Menu.Item>
                <Menu.Item leftSection={<IconTruckDelivery size={16} />} onClick={() => navigate('/dokumente/neu?type=delivery')}>
                  Lieferschein
                </Menu.Item>
                <Menu.Item leftSection={<IconReceiptRefund size={16} />} onClick={() => navigate('/dokumente/neu?type=credit_note')}>
                  Gutschrift
                </Menu.Item>
                <Menu.Divider />
                <Menu.Label>Einkauf & Zahlungen</Menu.Label>
                <Menu.Item leftSection={<IconReceipt2 size={16} />} onClick={() => navigate('/belege/neu')} rightSection={<Text size="xs" c="dimmed">{shortcutLabel('B', 'alt')}</Text>}>
                  Beleg erfassen
                </Menu.Item>
                <Menu.Item leftSection={<IconCash size={16} />} onClick={() => navigate('/kasse?neu=1')}>
                  Kassenbuchung
                </Menu.Item>
                <Menu.Item leftSection={<IconWritingSign size={16} />} onClick={() => navigate('/buchhaltung/journal?neu=1')}>
                  Manuelle Buchung
                </Menu.Item>
                <Menu.Divider />
                <Menu.Item leftSection={<IconUser size={16} />} onClick={() => navigate('/kontakte?neu=1')}>
                  Kontakt
                </Menu.Item>
                <Menu.Item leftSection={<IconPackage size={16} />} onClick={() => navigate('/artikel?neu=1')}>
                  Produkt / Leistung
                </Menu.Item>
              </Menu.Dropdown>
            </Menu>
            <ColorSchemeToggle />
            <Tooltip label={update ? 'Einstellungen · Update verfügbar' : 'Einstellungen'}>
              <Indicator disabled={!update} size={9} offset={5} color="brand">
                <ActionIcon size="lg" variant="default" onClick={() => navigate(update ? '/einstellungen/updates' : '/einstellungen')} aria-label="Einstellungen">
                  <IconSettings size={18} />
                </ActionIcon>
              </Indicator>
            </Tooltip>
          </Group>
        </Group>
        {hasTitlebar && <div style={{ width: 1, alignSelf: 'center', height: 24, marginLeft: 12, background: 'var(--app-border)', flexShrink: 0 }} />}
        <WindowControls />
      </AppShell.Header>

      <AppShell.Navbar p="xs">
        <AppShell.Section grow component={ScrollArea} type="scroll">
          <NavContent sections={sections} onNavigate={close} />
        </AppShell.Section>
        <AppShell.Section p="xs">
          <Text size="xs" c="dimmed">
            {status.bookkeeping === 'bilanz' ? 'Doppelte Buchführung' : 'Einnahmen-Überschuss-Rechnung'} · {status.chart}
            {status.small_business ? ' · Kleinunternehmer' : ''}
          </Text>
        </AppShell.Section>
      </AppShell.Navbar>

      <AppShell.Main>
        <Outlet />
      </AppShell.Main>
      <SearchSpotlight />
    </AppShell>
  );
}
