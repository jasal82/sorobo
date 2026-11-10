// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { useEffect, useState } from 'react';
import dayjs from 'dayjs';
import { useQueryClient } from '@tanstack/react-query';
import { useNavigate, useParams } from 'react-router';
import {
  ActionIcon,
  Alert,
  Badge,
  Box,
  Button,
  Card,
  Checkbox,
  FileButton,
  Grid,
  Group,
  Image,
  Menu,
  Modal,
  NumberInput,
  Paper,
  PasswordInput,
  Radio,
  SegmentedControl,
  Select,
  SimpleGrid,
  Stack,
  Switch,
  Table,
  Tabs,
  Text,
  TextInput,
  Textarea,
  ThemeIcon,
} from '@mantine/core';
import {
  IconAlertTriangle,
  IconArchive,
  IconBell,
  IconBuilding,
  IconBuildingBank,
  IconCopy,
  IconCloudDownload,
  IconDatabase,
  IconInfoCircle,
  IconDeviceFloppy,
  IconDots,
  IconEdit,
  IconFileText,
  IconHash,
  IconHome2,
  IconLayout,
  IconLock,
  IconMail,
  IconPlus,
  IconReceiptTax,
  IconRefresh,
  IconStar,
  IconTrash,
  IconUpload,
  IconCash,
} from '@tabler/icons-react';
import { api, apiUrl, downloadFile } from '@/api/client';
import { notifyError, useAction, useApi, useChart, useLayouts, usePaymentAccounts, useSettings, useStatus } from '@/api/hooks';
import type { BackupStatus, BankRule, DocumentListItem, FileMeta, Layout, MessageResponse, NumberRange, PaymentAccount, Settings, TransactionView } from '@/api/types';
import { AccountSelect, DateField, MoneyInput } from '@/components/inputs';
import { Page, PageHeader, SoroboLogo, UnsavedChangesGuard, confirmAction } from '@/components/ui';
import { date, dateTime, money, today } from '@/lib/format';
import { BOOKKEEPING, COUNTRIES, EINVOICE_FORMATS } from '@/lib/labels';
import { creditorIdValid } from '@/lib/sepa';
import { PaymentAccountModal } from '../banking/BankPage';
import { UpdatesTab } from './UpdatesTab';

type Patch = (fn: (s: Settings) => Settings) => void;

function CompanyTab({ s, patch }: { s: Settings; patch: Patch }) {
  const c = s.company;
  const set = (p: Partial<Settings['company']>) => patch((x) => ({ ...x, company: { ...x.company, ...p } }));
  const [uploading, setUploading] = useState(false);
  const uploadLogo = async (f: File | null) => {
    if (!f) return;
    setUploading(true);
    try {
      const meta = await api.upload<FileMeta>('/files', f);
      set({ logo_file_id: meta.id });
    } catch (e) {
      notifyError(e);
    } finally {
      setUploading(false);
    }
  };
  return (
    <Grid gap="lg">
      <Grid.Col span={{ base: 12, lg: 8 }}>
        <Card>
          <Stack gap="sm">
            <Text fw={650}>Firma & Anschrift</Text>
            <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="sm">
              <TextInput label="Firmenname" required value={c.name} onChange={(e) => set({ name: e.currentTarget.value })} />
              <Select label="Rechtsform" data={['Einzelunternehmen', 'Freiberufler', 'GbR', 'GmbH', 'UG (haftungsbeschränkt)', 'GmbH & Co. KG', 'KG', 'OHG', 'e.K.', 'AG', 'e.V.']} value={c.legal_form || null} onChange={(v) => set({ legal_form: v ?? '' })} searchable />
              <TextInput label="Inhaber / Geschäftsführung" value={c.owner} onChange={(e) => set({ owner: e.currentTarget.value })} />
              <TextInput label="Ansprechpartner (Rechnungen)" value={c.contact_name} onChange={(e) => set({ contact_name: e.currentTarget.value })} />
              <TextInput label="Straße und Hausnummer" value={c.street} onChange={(e) => set({ street: e.currentTarget.value })} />
              <SimpleGrid cols={2} spacing="sm">
                <TextInput label="PLZ" value={c.zip} onChange={(e) => set({ zip: e.currentTarget.value })} />
                <TextInput label="Ort" value={c.city} onChange={(e) => set({ city: e.currentTarget.value })} />
              </SimpleGrid>
              <Select label="Land" data={COUNTRIES} value={c.country} onChange={(v) => set({ country: v ?? 'DE' })} searchable />
              <TextInput label="Telefon" value={c.phone} onChange={(e) => set({ phone: e.currentTarget.value })} />
              <TextInput label="E-Mail" value={c.email} onChange={(e) => set({ email: e.currentTarget.value })} />
              <TextInput label="Website" value={c.website} onChange={(e) => set({ website: e.currentTarget.value })} />
            </SimpleGrid>
            <Text fw={650} mt="md">
              Steuern & Register
            </Text>
            <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="sm">
              <TextInput label="Steuernummer" value={c.tax_number} onChange={(e) => set({ tax_number: e.currentTarget.value })} />
              <TextInput label="USt-IdNr." value={c.vat_id} onChange={(e) => set({ vat_id: e.currentTarget.value })} />
              <TextInput label="Registergericht" value={c.register_court} onChange={(e) => set({ register_court: e.currentTarget.value })} />
              <TextInput label="Registernummer" value={c.register_number} onChange={(e) => set({ register_number: e.currentTarget.value })} />
            </SimpleGrid>
            <Text fw={650} mt="md">
              Bankverbindung (erscheint auf Rechnungen)
            </Text>
            <SimpleGrid cols={{ base: 1, sm: 3 }} spacing="sm">
              <TextInput label="Bank" value={c.bank_name} onChange={(e) => set({ bank_name: e.currentTarget.value })} />
              <TextInput label="IBAN" value={c.iban} onChange={(e) => set({ iban: e.currentTarget.value })} />
              <TextInput label="BIC" value={c.bic} onChange={(e) => set({ bic: e.currentTarget.value })} />
            </SimpleGrid>
            <Text fw={650} mt="md">
              SEPA-Lastschrift
            </Text>
            <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="sm">
              <TextInput
                label="Gläubiger-Identifikationsnummer"
                placeholder="DE98ZZZ09999999999"
                description="Bei der Deutschen Bundesbank zu beantragen; nötig, um Rechnungen per Lastschrift einzuziehen"
                value={s.debit.creditor_id}
                onChange={(e) => {
                  const v = e.currentTarget.value;
                  patch((x) => ({ ...x, debit: { ...x.debit, creditor_id: v } }));
                }}
                error={s.debit.creditor_id.trim() && !creditorIdValid(s.debit.creditor_id) ? 'Prüfziffer oder Format ungültig' : undefined}
              />
              <NumberInput
                label="Frist der Vorabankündigung"
                suffix=" Tage"
                min={1}
                max={30}
                description="14 Tage, sofern mit den Kunden keine kürzere Frist vereinbart ist (steht dann im Mandat)"
                value={s.debit.prenotification_days}
                onChange={(v) => patch((x) => ({ ...x, debit: { ...x.debit, prenotification_days: Number(v) || 14 } }))}
              />
            </SimpleGrid>
          </Stack>
        </Card>
      </Grid.Col>
      <Grid.Col span={{ base: 12, lg: 4 }}>
        <Card>
          <Text fw={650} mb="sm">
            Logo
          </Text>
          <Paper withBorder h={140} radius="md" p="md" style={{ display: 'flex', alignItems: 'center', justifyContent: 'center' }} bg="white">
            {c.logo_file_id ? <Image src={apiUrl(`/files/${c.logo_file_id}`)} mah={110} fit="contain" alt="Logo" /> : <Text size="sm" c="dimmed">Kein Logo hinterlegt</Text>}
          </Paper>
          <Group mt="sm">
            <FileButton onChange={uploadLogo} accept="image/png,image/jpeg,image/svg+xml,image/webp">
              {(props) => (
                <Button {...props} variant="light" leftSection={<IconUpload size={16} />} loading={uploading}>
                  Logo hochladen
                </Button>
              )}
            </FileButton>
            {c.logo_file_id && (
              <Button variant="subtle" color="red" onClick={() => set({ logo_file_id: null })}>
                Entfernen
              </Button>
            )}
          </Group>
          <Text size="xs" c="dimmed" mt="sm">
            PNG mit transparentem Hintergrund oder SVG empfohlen. Position und Größe legen Sie im Layout-Designer fest.
          </Text>
        </Card>
      </Grid.Col>
    </Grid>
  );
}

/** Legal forms that are merchants by law (Kaufleute) and must keep double-entry books. */
const MERCHANT_FORMS = ['GmbH', 'UG (haftungsbeschränkt)', 'GmbH & Co. KG', 'KG', 'OHG', 'e.K.', 'AG'];

/** Festschreibung: extends the locked period after a confirmation. It can never be lifted. */
function LockCard({ locked }: { locked: string | null }) {
  const [open, setOpen] = useState(false);
  const [until, setUntil] = useState<string | null>(null);
  const [understood, setUnderstood] = useState(false);
  const minDate = locked ? dayjs(locked).add(1, 'day').format('YYYY-MM-DD') : undefined;
  const lastMonthEnd = dayjs().subtract(1, 'month').endOf('month').format('YYYY-MM-DD');
  const show = () => {
    setUntil(!locked || lastMonthEnd > locked ? lastMonthEnd : null);
    setUnderstood(false);
    setOpen(true);
  };
  const valid = !!until && until <= today() && (!locked || until > locked);
  const { data: drafts } = useApi<DocumentListItem[]>('/documents', { status: 'draft', to: until }, { enabled: open && valid });
  const { data: openTx } = useApi<TransactionView[]>('/transactions', { status: 'open', to: until }, { enabled: open && valid });
  const draftCount = (drafts ?? []).filter((d) => d.doc_type === 'invoice' || d.doc_type === 'credit_note').length;
  const lock = useAction((d: string) => api.post<Settings>('/settings/lock', { until: d }), {
    success: (_r, d) => `Festgeschrieben bis einschließlich ${date(d)}`,
    onSuccess: () => setOpen(false),
  });
  return (
    <Card>
      <Group gap="sm" mb="sm">
        <ThemeIcon variant="light" color="orange">
          <IconLock size={16} />
        </ThemeIcon>
        <Text fw={650}>Festschreibung (GoBD)</Text>
        {locked && (
          <Badge variant="light" color="orange">
            bis {date(locked)}
          </Badge>
        )}
      </Group>
      <Text size="sm" c="dimmed" mb="sm">
        Festgeschriebene Zeiträume können nicht mehr verändert werden – z. B. nach Abgabe der Umsatzsteuer-Voranmeldung oder des Jahresabschlusses. Eine Festschreibung kann später nur verlängert, aber weder verkürzt noch aufgehoben werden.
      </Text>
      <Group justify="space-between" align="center">
        <Text size="sm">{locked ? <>Festgeschrieben bis einschließlich <b>{date(locked)}</b></> : 'Noch kein Zeitraum festgeschrieben.'}</Text>
        <Button variant="light" color="orange" leftSection={<IconLock size={16} />} onClick={show} disabled={!!locked && locked >= today()}>
          Zeitraum festschreiben …
        </Button>
      </Group>
      <Modal opened={open} onClose={() => setOpen(false)} title={<Text fw={650}>Zeitraum festschreiben</Text>}>
        <Stack>
          <DateField label="Festschreiben bis einschließlich" value={until} onChange={setUntil} minDate={minDate} maxDate={today()} required />
          {valid && draftCount > 0 && (
            <Alert color="yellow" icon={<IconAlertTriangle size={16} />} p="sm">
              <Text size="sm">
                {draftCount === 1 ? 'Ein Rechnungsentwurf' : `${draftCount} Rechnungsentwürfe`} mit Datum bis {date(until)} {draftCount === 1 ? 'kann' : 'können'} danach nicht mehr abgeschlossen werden.
              </Text>
            </Alert>
          )}
          {valid && (openTx?.length ?? 0) > 0 && (
            <Alert color="yellow" icon={<IconAlertTriangle size={16} />} p="sm">
              <Text size="sm">
                {openTx!.length === 1 ? 'Eine Kontobewegung' : `${openTx!.length} Kontobewegungen`} bis {date(until)} {openTx!.length === 1 ? 'ist' : 'sind'} noch nicht zugeordnet und {openTx!.length === 1 ? 'kann' : 'können'} danach nicht mehr gebucht werden.
              </Text>
            </Alert>
          )}
          <Checkbox
            label="Mir ist bewusst, dass Buchungen bis zu diesem Tag danach nicht mehr geändert werden können und die Festschreibung nicht rückgängig zu machen ist."
            checked={understood}
            onChange={(e) => setUnderstood(e.currentTarget.checked)}
          />
          <Group justify="flex-end">
            <Button variant="default" onClick={() => setOpen(false)}>
              Abbrechen
            </Button>
            <Button color="orange" leftSection={<IconLock size={16} />} disabled={!valid || !understood} loading={lock.isPending} onClick={() => until && lock.mutate(until)}>
              Festschreiben
            </Button>
          </Group>
        </Stack>
      </Modal>
    </Card>
  );
}

function TaxTab({ s, patch }: { s: Settings; patch: Patch }) {
  const { data: status } = useStatus();
  const { data: saved } = useSettings();
  const t = s.tax;
  // The lock is changed outside the form, so the saved state is authoritative.
  const locked = saved ? saved.tax.locked_until : t.locked_until;
  const set = (p: Partial<Settings['tax']>) => patch((x) => ({ ...x, tax: { ...x.tax, ...p } }));
  const rebuild = useAction(() => api.post<MessageResponse>('/system/rebuild'), { success: (r) => r.message });
  return (
    <Stack gap="lg" maw={900}>
      <Card>
        <Text fw={650} mb="sm">
          Buchführung
        </Text>
        <Radio.Group value={t.bookkeeping} onChange={(v) => set({ bookkeeping: v })}>
          <SimpleGrid cols={{ base: 1, sm: 2 }}>
            {BOOKKEEPING.map((b) => (
              <Radio.Card key={b.value} value={b.value} p="md" radius="md" disabled={!!locked}>
                <Group wrap="nowrap" align="flex-start">
                  <Radio.Indicator />
                  <div>
                    <Text fw={600} size="sm">
                      {b.label}
                    </Text>
                    <Text size="xs" c="dimmed" mt={4}>
                      {b.description}
                    </Text>
                  </div>
                </Group>
              </Radio.Card>
            ))}
          </SimpleGrid>
        </Radio.Group>
        {t.bookkeeping === 'euer' && MERCHANT_FORMS.includes(s.company.legal_form) && (
          <Alert color="yellow" icon={<IconAlertTriangle size={16} />} mt="md" p="sm">
            <Text size="sm">
              Als {s.company.legal_form} sind Sie nach § 238 HGB zur doppelten Buchführung verpflichtet. Die Einnahmen-Überschuss-Rechnung ist nur für Freiberufler, Kleingewerbetreibende und nicht eingetragene Einzelunternehmen zulässig.
            </Text>
          </Alert>
        )}
        <SimpleGrid cols={{ base: 1, sm: 2 }} mt="md" spacing="sm">
          <Select
            label="Kontenrahmen"
            data={[
              { value: 'SKR03', label: 'SKR03 (Prozessgliederung)' },
              { value: 'SKR04', label: 'SKR04 (Abschlussgliederung)' },
            ]}
            value={t.chart}
            onChange={(v) => set({ chart: v ?? 'SKR03' })}
            description={status?.chart_locked ? 'Kann nicht mehr geändert werden, sobald Zahlungskonten, Belege oder Buchungen angelegt sind' : undefined}
            disabled={status?.chart_locked}
          />
          <NumberInput label="Standard-Umsatzsteuersatz" suffix=" %" value={t.default_tax_rate} onChange={(v) => set({ default_tax_rate: Number(v) || 19 })} />
        </SimpleGrid>
      </Card>
      <Card>
        <Text fw={650} mb="sm">
          Umsatzsteuer
        </Text>
        <Stack gap="sm">
          <Switch label="Kleinunternehmer nach § 19 UStG" description="Es wird keine Umsatzsteuer ausgewiesen und keine Vorsteuer abgezogen" checked={t.small_business} onChange={(e) => set({ small_business: e.currentTarget.checked })} disabled={!!locked} />
          <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="sm">
            <Select
              label="Voranmeldungszeitraum"
              data={[
                { value: 'monthly', label: 'Monatlich' },
                { value: 'quarterly', label: 'Vierteljährlich' },
                { value: 'yearly', label: 'Jährlich (keine Voranmeldung)' },
              ]}
              value={t.ustva_period}
              onChange={(v) => set({ ustva_period: v ?? 'quarterly' })}
            />
            <Switch mt="lg" label="Dauerfristverlängerung" description="Fälligkeit verschiebt sich um einen Monat" checked={t.permanent_extension} onChange={(e) => set({ permanent_extension: e.currentTarget.checked })} />
            <Select
              label="Zusammenfassende Meldung"
              description="Monatlich, sobald die innergemeinschaftlichen Lieferungen 50.000 € im Quartal übersteigen"
              data={[
                { value: 'monthly', label: 'Monatlich' },
                { value: 'quarterly', label: 'Vierteljährlich' },
              ]}
              value={t.zm_period}
              onChange={(v) => set({ zm_period: v ?? 'quarterly' })}
            />
          </SimpleGrid>
          <Text size="xs" c="dimmed">
            Versteuerung: {t.bookkeeping === 'bilanz' ? 'Soll-Versteuerung (nach vereinbarten Entgelten)' : 'Ist-Versteuerung (nach vereinnahmten Entgelten)'}
          </Text>
        </Stack>
      </Card>
      <LockCard locked={locked} />
      <Card>
        <Text fw={650} mb={4}>
          Buchungen neu erzeugen
        </Text>
        <Text size="sm" c="dimmed" mb="sm">
          Erzeugt das Buchungsjournal aus allen Rechnungen, Belegen, Zahlungen und Anlagen neu. Buchungen festgeschriebener Zeiträume bleiben unverändert. Normalerweise nicht nötig.
        </Text>
        <Button variant="light" leftSection={<IconRefresh size={16} />} onClick={() => rebuild.mutate()} loading={rebuild.isPending}>
          Journal neu erzeugen
        </Button>
      </Card>
    </Stack>
  );
}

const TEXT_KEYS: { key: string; label: string }[] = [
  { key: 'invoice', label: 'Rechnung' },
  { key: 'invoice_debit', label: 'Rechnung (Lastschrift)' },
  { key: 'quote', label: 'Angebot' },
  { key: 'order', label: 'Auftragsbestätigung' },
  { key: 'delivery', label: 'Lieferschein' },
  { key: 'partial', label: 'Abschlagsrechnung' },
  { key: 'final', label: 'Schlussrechnung' },
  { key: 'credit_note', label: 'Gutschrift' },
  { key: 'cancellation', label: 'Stornorechnung' },
];

function DocsTab({ s, patch }: { s: Settings; patch: Patch }) {
  const { data: layouts } = useLayouts();
  const d = s.docs;
  const set = (p: Partial<Settings['docs']>) => patch((x) => ({ ...x, docs: { ...x.docs, ...p } }));
  const [key, setKey] = useState('invoice');
  const texts = d.texts[key] ?? { title: '', intro: '', closing: '' };
  const setText = (p: Partial<typeof texts>) => set({ texts: { ...d.texts, [key]: { ...texts, ...p } } });
  return (
    <Stack gap="lg" maw={1000}>
      <Card>
        <Text fw={650} mb="sm">
          Standardwerte
        </Text>
        <SimpleGrid cols={{ base: 1, sm: 3 }} spacing="sm">
          <NumberInput label="Zahlungsziel" suffix=" Tage" min={0} value={d.payment_terms_days} onChange={(v) => set({ payment_terms_days: Number(v) || 0 })} />
          <NumberInput label="Gültigkeit von Angeboten" suffix=" Tage" min={1} value={d.quote_validity_days} onChange={(v) => set({ quote_validity_days: Number(v) || 30 })} />
          <TextInput label="Standard-Einheit" value={d.default_unit} onChange={(e) => set({ default_unit: e.currentTarget.value })} />
          <Select label="Standard-Rechnungsformat" data={EINVOICE_FORMATS} value={d.einvoice_default} onChange={(v) => set({ einvoice_default: v ?? 'pdf' })} />
          <Select
            label="Standard-Layout"
            data={(layouts ?? []).map((l) => ({ value: String(l.id), label: l.name }))}
            value={String(d.default_layout_id ?? layouts?.find((l) => l.is_default)?.id ?? layouts?.[0]?.id ?? '') || null}
            onChange={(v) => set({ default_layout_id: v ? Number(v) : null })}
            allowDeselect={false}
          />
        </SimpleGrid>
      </Card>
      <Card>
        <Text fw={650} mb={4}>
          Textvorlagen
        </Text>
        <Text size="sm" c="dimmed" mb="sm">
          Platzhalter: {'{{Anrede}}, {{Kunde}}, {{Nummer}}, {{Datum}}, {{Betrag}}, {{Fällig am}}, {{Zahlungsziel}}, {{Gültig bis}}, {{Skonto}}, {{Firma}}, {{Referenz}}, {{Bezugsnummer}}, {{Leistungszeitraum}}, {{Monat}}, {{Quartal}}, {{Jahr}}'}
        </Text>
        <Tabs value={key} onChange={(v) => setKey(v ?? 'invoice')} variant="pills">
          <Tabs.List mb="md">
            {TEXT_KEYS.map((t) => (
              <Tabs.Tab key={t.key} value={t.key}>
                {t.label}
              </Tabs.Tab>
            ))}
          </Tabs.List>
        </Tabs>
        {key === 'invoice_debit' ? (
          <Textarea
            label="Schlusstext"
            description="Ersetzt bei Rechnungen per SEPA-Lastschrift den Schlusstext der Rechnung und dient als Vorabankündigung. Zusätzliche Platzhalter: {{Mandatsreferenz}}, {{Gläubiger-ID}}, {{Kunden-IBAN}}"
            minRows={4}
            value={texts.closing}
            onChange={(e) => setText({ closing: e.currentTarget.value })}
          />
        ) : (
          <Stack gap="sm">
            <TextInput label="Titel" value={texts.title} onChange={(e) => setText({ title: e.currentTarget.value })} />
            <Textarea label="Einleitungstext" minRows={4} value={texts.intro} onChange={(e) => setText({ intro: e.currentTarget.value })} />
            <Textarea label="Schlusstext" minRows={4} value={texts.closing} onChange={(e) => setText({ closing: e.currentTarget.value })} />
          </Stack>
        )}
      </Card>
    </Stack>
  );
}

function LayoutsTab() {
  const navigate = useNavigate();
  const { data: layouts } = useLayouts();
  const create = useAction((base?: Layout) => api.post<Layout>('/layouts', base ? { ...base, id: 0, name: `${base.name} (Kopie)`, is_default: false } : { id: 0, name: 'Neues Layout', is_default: false, config: layouts?.[0]?.config, created_at: '', updated_at: '' }), {
    success: 'Layout erstellt',
    onSuccess: (l) => navigate(`/einstellungen/layouts/${l.id}`),
  });
  const setDefault = useAction((id: number) => api.post(`/layouts/${id}/default`), { success: 'Standard-Layout geändert' });
  const remove = useAction((id: number) => api.del(`/layouts/${id}`), { success: 'Layout gelöscht' });
  return (
    <Stack gap="lg">
      <Group justify="space-between">
        <Text size="sm" c="dimmed">
          Gestalten Sie das Erscheinungsbild Ihrer Rechnungen, Angebote und Mahnungen – mit Logo, Briefpapier, Farben und frei positionierbaren Elementen.
        </Text>
        <Button leftSection={<IconPlus size={16} />} onClick={() => create.mutate(undefined)}>
          Neues Layout
        </Button>
      </Group>
      <SimpleGrid cols={{ base: 1, sm: 2, lg: 3 }} spacing="lg">
        {(layouts ?? []).map((l) => (
          <Card key={l.id} padding={0}>
            <Box h={180} p="md" bg="var(--app-bg)" style={{ cursor: 'pointer', borderBottom: '1px solid var(--app-border)' }} onClick={() => navigate(`/einstellungen/layouts/${l.id}`)}>
              <Paper h="100%" w={110} mx="auto" shadow="sm" radius={2} p={8} bg="white">
                {l.config.accent_bar && <Box h={4} bg={l.config.accent_color} mb={6} mx={-8} mt={-8} />}
                <Group justify="space-between" mb={10}>
                  <Box w={22} h={6} bg="#d1d5db" />
                  <Box w={30} h={10} bg={l.config.accent_color} style={{ borderRadius: 2, opacity: 0.85 }} />
                </Group>
                <Box w={40} h={4} bg="#e5e7eb" mb={3} />
                <Box w={34} h={4} bg="#e5e7eb" mb={10} />
                <Box w={50} h={6} bg="#374151" mb={8} />
                {[0, 1, 2, 3].map((i) => (
                  <Box key={i} h={4} mb={4} bg={i === 0 && l.config.table_header_fill ? l.config.accent_color : '#e5e7eb'} style={{ opacity: i === 0 ? 0.35 : 1 }} />
                ))}
                <Box w={36} h={5} ml="auto" bg="#374151" mt={6} />
              </Paper>
            </Box>
            <Group justify="space-between" p="md" wrap="nowrap" align="flex-start">
              <div style={{ minWidth: 0 }}>
                <Group gap={6} wrap="nowrap">
                  <Text fw={600} truncate>
                    {l.name}
                  </Text>
                  {l.is_default && (
                    <Badge size="sm" leftSection={<IconStar size={10} />} style={{ flexShrink: 0 }}>
                      Standard
                    </Badge>
                  )}
                </Group>
                <Text size="xs" c="dimmed">
                  {l.config.font} · {l.config.table_style}
                </Text>
              </div>
              <Group gap={4} wrap="nowrap" style={{ flexShrink: 0 }}>
                <Button size="compact-sm" variant="light" leftSection={<IconEdit size={14} />} onClick={() => navigate(`/einstellungen/layouts/${l.id}`)}>
                  Designer
                </Button>
                <Menu position="bottom-end">
                  <Menu.Target>
                    <ActionIcon aria-label="Mehr">
                      <IconDots size={16} />
                    </ActionIcon>
                  </Menu.Target>
                  <Menu.Dropdown>
                    {!l.is_default && (
                      <Menu.Item leftSection={<IconStar size={14} />} onClick={() => setDefault.mutate(l.id)}>
                        Als Standard festlegen
                      </Menu.Item>
                    )}
                    <Menu.Item leftSection={<IconCopy size={14} />} onClick={() => create.mutate(l)}>
                      Duplizieren
                    </Menu.Item>
                    <Menu.Item color="red" leftSection={<IconTrash size={14} />} onClick={() => confirmAction({ title: 'Layout löschen', message: `„${l.name}“ löschen?`, danger: true, confirmLabel: 'Löschen', onConfirm: () => remove.mutate(l.id) })}>
                      Löschen
                    </Menu.Item>
                  </Menu.Dropdown>
                </Menu>
              </Group>
            </Group>
          </Card>
        ))}
      </SimpleGrid>
    </Stack>
  );
}

function NumbersTab() {
  const { data } = useApi<NumberRange[]>('/number-ranges');
  const [rows, setRows] = useState<NumberRange[]>([]);
  useEffect(() => {
    if (data) setRows(data);
  }, [data]);
  const save = useAction(() => api.put('/number-ranges', rows), { success: 'Nummernkreise gespeichert' });
  const preview = (r: NumberRange) => {
    const d = new Date();
    let out = r.format.replace('{YYYY}', String(d.getFullYear())).replace('{YY}', String(d.getFullYear()).slice(2)).replace('{MM}', String(d.getMonth() + 1).padStart(2, '0'));
    out = out.replace(/\{(N+)\}/g, (_m, n: string) => String(r.next).padStart(n.length > 1 ? n.length : 0, '0'));
    return out;
  };
  return (
    <Card maw={1000}>
      <Text size="sm" c="dimmed" mb="md">
        Platzhalter: {'{YYYY}'} Jahr, {'{YY}'} Jahr zweistellig, {'{MM}'} Monat, {'{NNNN}'} fortlaufende Nummer (Anzahl N = Stellen). Rechnungsnummern werden lückenlos vergeben.
      </Text>
      <Table verticalSpacing="xs">
        <Table.Thead>
          <Table.Tr>
            <Table.Th>Nummernkreis</Table.Th>
            <Table.Th>Format</Table.Th>
            <Table.Th w={120}>Nächste Nr.</Table.Th>
            <Table.Th w={140}>Jährlich neu</Table.Th>
            <Table.Th>Vorschau</Table.Th>
          </Table.Tr>
        </Table.Thead>
        <Table.Tbody>
          {rows.map((r, i) => (
            <Table.Tr key={r.key}>
              <Table.Td>
                <Text size="sm" fw={500}>
                  {r.label}
                </Text>
              </Table.Td>
              <Table.Td>
                <TextInput size="xs" value={r.format} onChange={(e) => setRows(rows.map((x, j) => (j === i ? { ...x, format: e.currentTarget.value } : x)))} />
              </Table.Td>
              <Table.Td>
                <NumberInput size="xs" min={1} thousandSeparator="" allowDecimal={false} value={r.next} onChange={(v) => setRows(rows.map((x, j) => (j === i ? { ...x, next: Number(v) || 1 } : x)))} />
              </Table.Td>
              <Table.Td>
                <Switch size="xs" checked={r.reset_yearly} onChange={(e) => setRows(rows.map((x, j) => (j === i ? { ...x, reset_yearly: e.currentTarget.checked } : x)))} />
              </Table.Td>
              <Table.Td>
                <Badge variant="light" color="gray" size="lg" radius="sm" tt="none">
                  {preview(r)}
                </Badge>
              </Table.Td>
            </Table.Tr>
          ))}
        </Table.Tbody>
      </Table>
      <Group justify="flex-end" mt="md">
        <Button onClick={() => save.mutate()} loading={save.isPending} leftSection={<IconDeviceFloppy size={16} />}>
          Nummernkreise speichern
        </Button>
      </Group>
    </Card>
  );
}

function DunningTab({ s, patch }: { s: Settings; patch: Patch }) {
  const d = s.dunning;
  const set = (p: Partial<Settings['dunning']>) => patch((x) => ({ ...x, dunning: { ...x.dunning, ...p } }));
  const setLevel = (i: number, p: Partial<Settings['dunning']['levels'][number]>) => set({ levels: d.levels.map((l, j) => (j === i ? { ...l, ...p } : l)) });
  // The Basiszinssatz changes on 1 January and 1 July.
  const lastChange = dayjs().format(dayjs().month() >= 6 ? 'YYYY-07-01' : 'YYYY-01-01');
  const baseRateOutdated = !d.base_rate_since || d.base_rate_since < lastChange;
  return (
    <Stack gap="lg" maw={1000}>
      {d.levels.map((l, i) => (
        <Card key={i}>
          <Group justify="space-between" mb="sm">
            <Group gap="sm">
              <Badge color={['blue', 'yellow', 'orange', 'red'][i] ?? 'red'} size="lg" variant="light">
                Stufe {l.level}
              </Badge>
              <TextInput value={l.name} onChange={(e) => setLevel(i, { name: e.currentTarget.value, title: e.currentTarget.value })} variant="unstyled" fw={650} size="md" />
            </Group>
          </Group>
          <SimpleGrid cols={{ base: 1, sm: 3 }} spacing="sm" mb="sm">
            <NumberInput label={i === 0 ? 'Tage nach Fälligkeit' : 'Tage nach Ablauf der vorherigen Frist'} min={0} value={l.days} onChange={(v) => setLevel(i, { days: Number(v) || 0 })} />
            <NumberInput label="Neue Zahlungsfrist (Tage)" min={1} value={l.deadline_days} onChange={(v) => setLevel(i, { deadline_days: Number(v) || 10 })} />
            <MoneyInput label="Mahngebühr" value={l.fee} onChange={(v) => setLevel(i, { fee: v })} />
          </SimpleGrid>
          <Textarea label="Text" minRows={5} value={l.text} onChange={(e) => setLevel(i, { text: e.currentTarget.value })} description="Platzhalter: {{Anrede}}, {{Rechnungsnummer}}, {{Rechnungsdatum}}, {{Offener Betrag}}, {{Mahngebühr}}, {{Zinsen}}, {{Gesamtbetrag}}, {{Neue Frist}}, {{Firma}}" />
        </Card>
      ))}
      <Card>
        <Text fw={650} mb="sm">
          Verzugszinsen (§ 288 BGB)
        </Text>
        <Switch label="Verzugszinsen ab der 1. Mahnung berechnen" checked={d.interest_enabled} onChange={(e) => set({ interest_enabled: e.currentTarget.checked })} mb="sm" />
        {d.interest_enabled && baseRateOutdated && (
          <Alert color="orange" icon={<IconAlertTriangle size={16} />} p="sm" mb="sm">
            <Text size="sm">
              Der Basiszinssatz ändert sich zum 1. Januar und 1. Juli. Bitte den aktuellen Wert bei der Deutschen Bundesbank prüfen und mit dem Datum eintragen, ab dem er gilt.
            </Text>
          </Alert>
        )}
        <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="sm">
          <NumberInput label="Basiszinssatz" suffix=" %" decimalScale={2} decimalSeparator="," value={d.base_rate} onChange={(v) => set({ base_rate: Number(v) || 0 })} description="Veröffentlicht von der Deutschen Bundesbank" />
          <DateField label="gültig seit" value={d.base_rate_since} onChange={(v) => set({ base_rate_since: v })} clearable />
          <NumberInput label="Aufschlag Unternehmen" suffix=" %-Punkte" value={d.surcharge_b2b} onChange={(v) => set({ surcharge_b2b: Number(v) || 0 })} />
          <NumberInput label="Aufschlag Verbraucher" suffix=" %-Punkte" value={d.surcharge_b2c} onChange={(v) => set({ surcharge_b2c: Number(v) || 0 })} />
        </SimpleGrid>
      </Card>
      <Card>
        <Text fw={650} mb="sm">
          Verzugspauschale (§ 288 Abs. 5 BGB)
        </Text>
        <Switch
          label="Unternehmen ab der 1. Mahnung 40 € Verzugspauschale berechnen"
          description="Die Pauschale wird auf die Mahngebühren angerechnet: Insgesamt wird der höhere Betrag aus Pauschale und Mahngebühren berechnet. Verbraucher sind ausgenommen."
          checked={d.flat_fee_b2b}
          onChange={(e) => set({ flat_fee_b2b: e.currentTarget.checked })}
        />
      </Card>
    </Stack>
  );
}

const EMPTY_RULE: BankRule = { id: 0, name: '', counterparty: '', iban: '', purpose: '', direction: 'out', account: '', tax_rate: 19, description: '', active: true, created_at: '' };

function BankRuleModal({ rule, onClose }: { rule: BankRule | null; onClose: () => void }) {
  const [r, setR] = useState<BankRule>(EMPTY_RULE);
  useEffect(() => {
    if (rule) setR(rule);
  }, [rule]);
  const set = (p: Partial<BankRule>) => setR((x) => ({ ...x, ...p }));
  const save = useAction((x: BankRule) => (x.id ? api.put(`/bank-rules/${x.id}`, x) : api.post('/bank-rules', x)), { success: 'Regel gespeichert', onSuccess: onClose });
  return (
    <Modal opened={!!rule} onClose={onClose} title={<Text fw={650}>{r.id ? 'Bankregel bearbeiten' : 'Neue Bankregel'}</Text>}>
      <Stack gap="sm">
        <Text size="sm" c="dimmed">
          Importierte Kontobewegungen, auf die alle ausgefüllten Merkmale zutreffen, werden automatisch auf das Konto gebucht.
        </Text>
        <TextInput label="Name enthält" placeholder="z. B. Telekom" value={r.counterparty} onChange={(e) => set({ counterparty: e.currentTarget.value })} />
        <TextInput label="IBAN" value={r.iban} onChange={(e) => set({ iban: e.currentTarget.value })} />
        <TextInput label="Verwendungszweck enthält" value={r.purpose} onChange={(e) => set({ purpose: e.currentTarget.value })} />
        <SegmentedControl
          value={r.direction}
          onChange={(v) => set({ direction: v })}
          data={[
            { value: 'out', label: 'Ausgaben' },
            { value: 'in', label: 'Einnahmen' },
            { value: '', label: 'Beide' },
          ]}
        />
        <SimpleGrid cols={2} spacing="sm">
          <AccountSelect label="Konto" value={r.account} onChange={(v) => set({ account: v ?? '' })} allAccounts />
          <Select
            label="Umsatzsteuer"
            data={[
              { value: '19', label: '19 %' },
              { value: '7', label: '7 %' },
              { value: '0', label: 'ohne' },
            ]}
            value={String(r.tax_rate)}
            onChange={(v) => set({ tax_rate: Number(v ?? 0) })}
          />
        </SimpleGrid>
        <TextInput label="Buchungstext" placeholder="Verwendungszweck der Bewegung" value={r.description} onChange={(e) => set({ description: e.currentTarget.value })} />
        <Group justify="flex-end">
          <Button variant="default" onClick={onClose}>
            Abbrechen
          </Button>
          <Button onClick={() => save.mutate(r)} loading={save.isPending}>
            Speichern
          </Button>
        </Group>
      </Stack>
    </Modal>
  );
}

function BankRulesCard() {
  const { data: rules } = useApi<BankRule[]>('/bank-rules');
  const { data: chart } = useChart();
  const [editing, setEditing] = useState<BankRule | null>(null);
  const toggle = useAction((r: BankRule) => api.put(`/bank-rules/${r.id}`, { ...r, active: !r.active }));
  const remove = useAction((id: number) => api.del(`/bank-rules/${id}`), { success: 'Regel gelöscht' });
  const apply = useAction(() => api.post<MessageResponse>('/bank-rules/apply'), { success: (r) => r.message });
  const criteria = (r: BankRule) =>
    [r.counterparty && `Name enthält „${r.counterparty}“`, r.iban && `IBAN ${r.iban}`, r.purpose && `Zweck enthält „${r.purpose}“`].filter(Boolean).join(' · ');
  return (
    <Card>
      <Group justify="space-between" mb="xs">
        <Text fw={650}>Bankregeln</Text>
        <Group gap="xs">
          <Button variant="default" size="compact-sm" onClick={() => apply.mutate()} loading={apply.isPending} disabled={!rules?.length}>
            Auf offene Umsätze anwenden
          </Button>
          <Button size="compact-sm" leftSection={<IconPlus size={14} />} onClick={() => setEditing(EMPTY_RULE)}>
            Regel anlegen
          </Button>
        </Group>
      </Group>
      <Text size="sm" c="dimmed" mb="sm">
        Wiederkehrende Kontobewegungen (z. B. Telefon, Miete) werden beim Import automatisch gebucht. Regeln lassen sich auch beim Zuordnen einer Bewegung anlegen.
      </Text>
      {!rules?.length ? (
        <Text size="sm" c="dimmed">
          Noch keine Regeln angelegt.
        </Text>
      ) : (
        <Table verticalSpacing="xs">
          <Table.Tbody>
            {rules.map((r) => (
              <Table.Tr key={r.id} style={{ opacity: r.active ? 1 : 0.5 }}>
                <Table.Td>
                  <Text size="sm" fw={500}>
                    {criteria(r)}
                  </Text>
                  <Text size="xs" c="dimmed">
                    {r.direction === 'in' ? 'Einnahmen' : r.direction === 'out' ? 'Ausgaben' : 'Einnahmen und Ausgaben'} → {r.account} {chart?.find((a) => a.number === r.account)?.name ?? ''}
                    {r.tax_rate > 0 ? ` · ${r.tax_rate} % USt` : ''}
                  </Text>
                </Table.Td>
                <Table.Td w={170}>
                  <Group gap={4} justify="flex-end" wrap="nowrap">
                    <Switch size="xs" checked={r.active} onChange={() => toggle.mutate(r)} aria-label="Aktiv" />
                    <ActionIcon variant="subtle" onClick={() => setEditing(r)} aria-label="Regel bearbeiten">
                      <IconEdit size={16} />
                    </ActionIcon>
                    <ActionIcon variant="subtle" color="red" onClick={() => confirmAction({ title: 'Regel löschen', message: `Die Regel „${criteria(r)}“ wird gelöscht.`, danger: true, confirmLabel: 'Löschen', onConfirm: () => remove.mutate(r.id) })} aria-label="Regel löschen">
                      <IconTrash size={16} />
                    </ActionIcon>
                  </Group>
                </Table.Td>
              </Table.Tr>
            ))}
          </Table.Tbody>
        </Table>
      )}
      <BankRuleModal rule={editing} onClose={() => setEditing(null)} />
    </Card>
  );
}

/** Sending documents via the user's SMTP server. */
function EmailTab({ s, patch }: { s: Settings; patch: Patch }) {
  const e = s.email;
  const set = (p: Partial<Settings['email']>) => patch((x) => ({ ...x, email: { ...x.email, ...p } }));
  const test = useAction(() => api.post<MessageResponse>('/settings/email-test'), { success: (r) => r.message, invalidate: false, errorTitle: 'Test-E-Mail fehlgeschlagen' });
  return (
    <Stack gap="lg" maw={900}>
      <Card>
        <Text fw={650} mb={4}>
          E-Mail-Versand
        </Text>
        <Text size="sm" c="dimmed" mb="sm">
          Dokumente direkt aus Sorobo versenden – mit PDF im Anhang, bei Kunden mit XRechnung zusätzlich mit der XML-Datei. Ohne eigenen Versand öffnet „Senden“ Ihr E-Mail-Programm.
        </Text>
        <Switch label="E-Mails über den eigenen E-Mail-Server senden" checked={e.enabled} onChange={(ev) => set({ enabled: ev.currentTarget.checked })} />
      </Card>
      <Card>
        <Text fw={650} mb="sm">
          Server (SMTP)
        </Text>
        <SimpleGrid cols={{ base: 1, sm: 3 }} spacing="sm">
          <TextInput label="Server" placeholder="smtp.example.org" value={e.host} onChange={(ev) => set({ host: ev.currentTarget.value })} />
          <Select
            label="Verschlüsselung"
            data={[
              { value: 'starttls', label: 'STARTTLS' },
              { value: 'tls', label: 'SSL/TLS' },
              { value: 'none', label: 'keine' },
            ]}
            value={e.security}
            onChange={(v) => set({ security: v ?? 'starttls', port: v === 'tls' ? 465 : v === 'starttls' ? 587 : e.port })}
          />
          <NumberInput label="Port" min={1} max={65535} value={e.port} onChange={(v) => set({ port: Number(v) || 587 })} />
          <TextInput label="Benutzername" value={e.username} onChange={(ev) => set({ username: ev.currentTarget.value })} />
          <PasswordInput
            label="Passwort"
            placeholder={e.has_password ? 'gespeichert' : ''}
            description={e.has_password ? 'Leer lassen, um das gespeicherte Passwort zu behalten' : undefined}
            value={e.password}
            onChange={(ev) => set({ password: ev.currentTarget.value })}
          />
        </SimpleGrid>
        <Text size="xs" c="dimmed" mt="xs">
          Das Passwort wird lokal in der Sorobo-Datenbank gespeichert und ist damit auch in Datensicherungen enthalten.
        </Text>
      </Card>
      <Card>
        <Text fw={650} mb="sm">
          Absender
        </Text>
        <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="sm">
          <TextInput label="Name" placeholder={s.company.name} value={e.from_name} onChange={(ev) => set({ from_name: ev.currentTarget.value })} />
          <TextInput label="E-Mail-Adresse" placeholder={s.company.email} value={e.from_address} onChange={(ev) => set({ from_address: ev.currentTarget.value })} />
        </SimpleGrid>
        <Switch mt="sm" label="Kopie an die Absenderadresse senden (BCC)" checked={e.bcc_self} onChange={(ev) => set({ bcc_self: ev.currentTarget.checked })} />
      </Card>
      <Card>
        <Text fw={650} mb="sm">
          Vorlage
        </Text>
        <Stack gap="sm">
          <TextInput label="Betreff" value={e.subject} onChange={(ev) => set({ subject: ev.currentTarget.value })} />
          <Textarea
            label="Text"
            autosize
            minRows={6}
            value={e.body}
            onChange={(ev) => set({ body: ev.currentTarget.value })}
            description="Platzhalter: {{Dokument}}, {{Nummer}}, {{Datum}}, {{Betrag}}, {{Fällig am}}, {{Anrede}}, {{Kunde}}, {{Ansprechpartner}}, {{Firma}}, {{Leistungszeitraum}}, {{Monat}}"
          />
        </Stack>
      </Card>
      <Group>
        <Button variant="light" leftSection={<IconMail size={16} />} onClick={() => test.mutate()} loading={test.isPending}>
          Test-E-Mail senden
        </Button>
        <Text size="xs" c="dimmed">
          Verwendet die gespeicherten Einstellungen und sendet an die Absenderadresse.
        </Text>
      </Group>
    </Stack>
  );
}

function AccountsTab() {
  const { data } = usePaymentAccounts();
  const [modal, setModal] = useState<{ open: boolean; account: PaymentAccount | null }>({ open: false, account: null });
  return (
    <Stack gap="md" maw={900}>
      <Group justify="flex-end">
        <Button leftSection={<IconPlus size={16} />} onClick={() => setModal({ open: true, account: null })}>
          Zahlungskonto hinzufügen
        </Button>
      </Group>
      {(data ?? []).map((a) => (
        <Card key={a.account.id} padding="md">
          <Group justify="space-between">
            <Group>
              <ThemeIcon variant="light" color={a.account.kind === 'cash' ? 'teal' : 'brand'}>
                {a.account.kind === 'cash' ? <IconCash size={16} /> : <IconBuildingBank size={16} />}
              </ThemeIcon>
              <div>
                <Text fw={600}>
                  {a.account.name} {a.account.archived && <Badge color="gray" size="xs">archiviert</Badge>}
                </Text>
                <Text size="xs" c="dimmed">
                  Sachkonto {a.account.ledger_account} · Anfangsbestand {money(a.account.opening_balance)} zum {date(a.account.opening_date)}
                </Text>
              </div>
            </Group>
            <Group>
              <Text fw={700} className="tabular">
                {money(a.balance)}
              </Text>
              <Button size="compact-sm" variant="subtle" onClick={() => setModal({ open: true, account: a.account })}>
                Bearbeiten
              </Button>
            </Group>
          </Group>
        </Card>
      ))}
      <PaymentAccountModal opened={modal.open} onClose={() => setModal({ open: false, account: null })} account={modal.account} />
      <BankRulesCard />
    </Stack>
  );
}

/** Daily automatic backup (settings are saved with the form, the status comes from the backend). */
function AutoBackupCard({ s, patch }: { s: Settings; patch: Patch }) {
  const b = s.backup;
  const set = (p: Partial<Settings['backup']>) => patch((x) => ({ ...x, backup: { ...x.backup, ...p } }));
  const { data: info } = useApi<BackupStatus>('/system/backups');
  const run = useAction(() => api.post<BackupStatus>('/system/backups'), { success: (r) => `Sicherung erstellt in ${r.folder}` });
  return (
    <Card>
      <Group gap="sm" mb="xs">
        <IconRefresh size={18} />
        <Text fw={650}>Automatische Sicherung</Text>
      </Group>
      <Text size="sm" c="dimmed" mb="sm">
        Sorobo legt einmal täglich eine vollständige Sicherung an und behält die neuesten. Zum Schutz vor einem Festplattenausfall einen Ordner auf einem anderen Laufwerk oder in einem Cloud-Ordner wählen.
      </Text>
      <Stack gap="sm">
        <Switch label="Täglich automatisch sichern" checked={b.enabled} onChange={(e) => set({ enabled: e.currentTarget.checked })} />
        <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="sm">
          <TextInput
            label="Ordner"
            placeholder={info?.default_folder}
            description="Leer lassen für „backups“ im Datenordner"
            value={b.folder}
            onChange={(e) => set({ folder: e.currentTarget.value })}
          />
          <NumberInput label="Aufbewahrte Sicherungen" min={1} max={365} value={b.keep} onChange={(v) => set({ keep: Number(v) || 30 })} />
        </SimpleGrid>
        <Group justify="space-between">
          <Text size="sm" c="dimmed">
            {info?.last_at ? `Letzte Sicherung: ${dateTime(info.last_at)}` : 'Noch keine automatische Sicherung'}
            {info ? ` · ${info.count} im Ordner ${info.folder}` : ''}
          </Text>
          <Button variant="light" leftSection={<IconArchive size={16} />} onClick={() => run.mutate()} loading={run.isPending}>
            Jetzt sichern
          </Button>
        </Group>
      </Stack>
    </Card>
  );
}

function DataTab({ s, patch }: { s: Settings; patch: Patch }) {
  const navigate = useNavigate();
  const { data: status } = useStatus();
  const [resetOpen, setResetOpen] = useState(false);
  const [confirmText, setConfirmText] = useState('');
  const [restoring, setRestoring] = useState(false);
  const demo = useAction(() => api.post('/system/demo'), { success: 'Demodaten geladen' });
  const qc = useQueryClient();
  const reset = useAction(() => api.post('/system/reset', { confirm: confirmText }), {
    success: 'Alle Daten wurden gelöscht',
    invalidate: false,
    onSuccess: () => {
      setResetOpen(false);
      setConfirmText('');
      // Remove all cached data – otherwise the welcome wizard would show the deleted company.
      qc.clear();
      navigate('/willkommen', { replace: true });
    },
  });
  const restore = async (f: File | null) => {
    if (!f) return;
    setRestoring(true);
    try {
      await api.upload('/system/restore', f);
      window.location.href = '/';
    } catch (e) {
      notifyError(e, 'Wiederherstellung fehlgeschlagen');
    } finally {
      setRestoring(false);
    }
  };
  return (
    <Stack gap="lg" maw={900}>
      <Card>
        <Group gap="sm" mb="xs">
          <IconDatabase size={18} />
          <Text fw={650}>Datensicherung</Text>
        </Group>
        <Text size="sm" c="dimmed" mb="sm">
          Alle Daten liegen lokal in <code>{status?.data_dir}</code>. Erstellen Sie regelmäßig Sicherungen.
        </Text>
        <Group>
          <Button leftSection={<IconArchive size={16} />} onClick={() => downloadFile('/system/backup').catch((e) => notifyError(e))}>
            Sicherung herunterladen
          </Button>
          <FileButton onChange={(f) => f && confirmAction({ title: 'Sicherung wiederherstellen', message: 'Alle aktuellen Daten werden durch die Sicherung ersetzt. Der bisherige Stand wird vorher im Datenordner unter „backups“ gesichert.', danger: true, confirmLabel: 'Wiederherstellen', onConfirm: () => restore(f) })} accept=".zip,application/zip">
            {(props) => (
              <Button {...props} variant="default" leftSection={<IconUpload size={16} />} loading={restoring}>
                Sicherung wiederherstellen
              </Button>
            )}
          </FileButton>
        </Group>
      </Card>
      <AutoBackupCard s={s} patch={patch} />
      <Card>
        <Text fw={650} mb={4}>
          Demodaten
        </Text>
        <Text size="sm" c="dimmed" mb="sm">
          Lädt ein realistisches Geschäftsjahr mit Kunden, Rechnungen, Belegen, Zahlungen und Anlagen – ideal zum Ausprobieren.
        </Text>
        <Group gap="sm">
          <Button variant="light" onClick={() => demo.mutate()} loading={demo.isPending} disabled={status?.has_data} w="fit-content">
            Demodaten laden
          </Button>
          {status?.has_data && (
            <Text size="xs" c="dimmed">
              Nur bei leerem Datenbestand möglich – setzen Sie dazu vorher alle Daten zurück.
            </Text>
          )}
        </Group>
      </Card>
      <Card style={{ borderColor: 'var(--mantine-color-red-3)' }}>
        <Group gap="sm" mb={4}>
          <IconAlertTriangle size={18} color="var(--mantine-color-red-6)" />
          <Text fw={650} c="red.7">
            Alle Daten löschen
          </Text>
        </Group>
        <Text size="sm" c="dimmed" mb="sm">
          Setzt die Software vollständig zurück. Erstellen Sie vorher eine Sicherung!
        </Text>
        <Button color="red" variant="light" onClick={() => setResetOpen(true)} w="fit-content">
          Zurücksetzen …
        </Button>
      </Card>
      <Modal opened={resetOpen} onClose={() => setResetOpen(false)} title={<Text fw={650}>Alle Daten löschen</Text>}>
        <Stack>
          <Alert color="red" icon={<IconAlertTriangle size={16} />}>
            Alle Kontakte, Dokumente, Belege, Buchungen und Einstellungen werden unwiderruflich gelöscht.
          </Alert>
          <TextInput label="Zur Bestätigung LÖSCHEN eingeben" value={confirmText} onChange={(e) => setConfirmText(e.currentTarget.value)} />
          <Group justify="flex-end">
            <Button variant="default" onClick={() => setResetOpen(false)}>
              Abbrechen
            </Button>
            <Button color="red" disabled={confirmText !== 'LÖSCHEN'} onClick={() => reset.mutate()} loading={reset.isPending}>
              Endgültig löschen
            </Button>
          </Group>
        </Stack>
      </Modal>
    </Stack>
  );
}

function OverviewTab({ s, patch }: { s: Settings; patch: Patch }) {
  return (
    <Stack gap="lg" maw={760}>
      <Card>
        <Text fw={650} mb={4}>
          Startseite
        </Text>
        <Text size="sm" c="dimmed" mb="md">
          Legt fest, welche Bereiche auf der Übersicht angezeigt werden.
        </Text>
        <Switch
          label="„Erste Schritte“ anzeigen"
          description="Checkliste zur Einrichtung (Firmendaten, Logo, erster Kunde, erste Rechnung …). Sie verschwindet automatisch, sobald die wichtigsten Schritte erledigt sind."
          checked={!s.hide_getting_started}
          onChange={(e) => {
            const show = e.currentTarget.checked;
            patch((x) => ({ ...x, hide_getting_started: !show }));
          }}
        />
      </Card>
    </Stack>
  );
}

/** Public repository; shown as source code link in "Über Sorobo" once set. */
const SOURCE_URL = '';

/** Appropriate legal notices (AGPL-3.0 § 5 d): copyright, no warranty, licence. */
function AboutTab() {
  const { data: status } = useStatus();
  return (
    <Stack gap="lg" maw={760}>
      <Card>
        <Group gap="md" mb="md">
          <SoroboLogo size={48} />
          <div>
            <Text fw={700} size="lg">
              Sorobo
            </Text>
            <Text size="sm" c="dimmed">
              Version {status?.version ?? '–'} · lokale Buchhaltungs- und Rechnungssoftware
            </Text>
          </div>
        </Group>
        <Stack gap="sm">
          <Text size="sm">Copyright © 2026 Johannes Asal</Text>
          <Text size="sm">
            Sorobo ist freie Software: Sie können es unter den Bedingungen der GNU Affero General Public License, wie von der Free Software Foundation veröffentlicht, weitergeben und/oder
            verändern – entweder gemäß Version 3 der Lizenz oder (nach Ihrer Wahl) jeder späteren Version.
          </Text>
          <Text size="sm">
            Sorobo wird in der Hoffnung bereitgestellt, dass es nützlich ist, aber <b>ohne jede Gewährleistung</b> – sogar ohne die implizite Gewährleistung der Marktreife oder der Eignung für
            einen bestimmten Zweck. Die Software unterstützt bei der Buchführung, ersetzt aber keine steuerliche Beratung.
          </Text>
          <Group gap="sm" mt="xs">
            <Button component="a" href="https://www.gnu.org/licenses/agpl-3.0.html" target="_blank" rel="noreferrer" variant="default" size="xs">
              Lizenztext (AGPL-3.0)
            </Button>
            {SOURCE_URL && (
              <Button component="a" href={SOURCE_URL} target="_blank" rel="noreferrer" variant="default" size="xs">
                Quellcode
              </Button>
            )}
          </Group>
        </Stack>
      </Card>
      <Card>
        <Text fw={650} mb={4}>
          Komponenten Dritter
        </Text>
        <Text size="sm" c="dimmed">
          Sorobo enthält Bibliotheken und Schriften anderer Projekte unter deren eigenen, freien Lizenzen (u. a. MIT, Apache-2.0, BSD und SIL Open Font License). Die vollständigen Lizenztexte
          liegen in der Datei THIRD-PARTY-LICENSES.txt neben dem Programm.
        </Text>
      </Card>
    </Stack>
  );
}

const TABS = [
  { value: 'unternehmen', label: 'Unternehmen', icon: IconBuilding, saves: true },
  { value: 'steuern', label: 'Steuern & Buchführung', icon: IconReceiptTax, saves: true },
  { value: 'dokumente', label: 'Dokumente & Texte', icon: IconFileText, saves: true },
  { value: 'email', label: 'E-Mail', icon: IconMail, saves: true },
  { value: 'layouts', label: 'Layouts', icon: IconLayout, saves: false },
  { value: 'nummern', label: 'Nummernkreise', icon: IconHash, saves: false },
  { value: 'mahnwesen', label: 'Mahnwesen', icon: IconBell, saves: true },
  { value: 'konten', label: 'Zahlungskonten', icon: IconBuildingBank, saves: false },
  { value: 'uebersicht', label: 'Übersicht', icon: IconHome2, saves: true },
  { value: 'daten', label: 'Daten & Sicherung', icon: IconDatabase, saves: true },
  { value: 'updates', label: 'Updates', icon: IconCloudDownload, saves: true },
  { value: 'info', label: 'Über Sorobo', icon: IconInfoCircle, saves: false },
];

export function SettingsPage() {
  const { tab = 'unternehmen' } = useParams();
  const navigate = useNavigate();
  const { data } = useSettings();
  const [s, setS] = useState<Settings | null>(null);
  const [dirty, setDirty] = useState(false);
  useEffect(() => {
    if (data && !dirty) setS(data);
  }, [data, dirty]);
  const patch: Patch = (fn) => {
    setS((x) => (x ? fn(x) : x));
    setDirty(true);
  };
  const save = useAction((x: Settings) => api.put<Settings>('/settings', x), {
    success: 'Einstellungen gespeichert',
    onSuccess: (r) => {
      setS(r);
      setDirty(false);
    },
  });
  const current = TABS.find((t) => t.value === tab) ?? TABS[0];
  return (
    <Page>
      <PageHeader
        title="Einstellungen"
        actions={
          current.saves && (
            <Button leftSection={<IconDeviceFloppy size={16} />} onClick={() => s && save.mutate(s)} loading={save.isPending} disabled={!dirty}>
              Änderungen speichern
            </Button>
          )
        }
      />
      <Tabs value={current.value} onChange={(v) => navigate(`/einstellungen/${v}`)} orientation="horizontal">
        <Tabs.List mb="lg">
          {TABS.map((t) => (
            <Tabs.Tab key={t.value} value={t.value} leftSection={<t.icon size={16} />}>
              {t.label}
            </Tabs.Tab>
          ))}
        </Tabs.List>
      </Tabs>
      {s && (
        <>
          {current.value === 'unternehmen' && <CompanyTab s={s} patch={patch} />}
          {current.value === 'steuern' && <TaxTab s={s} patch={patch} />}
          {current.value === 'dokumente' && <DocsTab s={s} patch={patch} />}
          {current.value === 'email' && <EmailTab s={s} patch={patch} />}
          {current.value === 'layouts' && <LayoutsTab />}
          {current.value === 'nummern' && <NumbersTab />}
          {current.value === 'mahnwesen' && <DunningTab s={s} patch={patch} />}
          {current.value === 'konten' && <AccountsTab />}
          {current.value === 'uebersicht' && <OverviewTab s={s} patch={patch} />}
          {current.value === 'daten' && <DataTab s={s} patch={patch} />}
          {current.value === 'updates' && <UpdatesTab s={s} patch={patch} />}
          {current.value === 'info' && <AboutTab />}
        </>
      )}
      <UnsavedChangesGuard when={dirty} onSave={() => (s ? save.mutateAsync(s) : Promise.resolve())} allow={(path) => path === '/willkommen' || /^\/einstellungen(\/[a-z]+)?$/.test(path)} />
    </Page>
  );
}
