// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { useState } from 'react';
import { useNavigate } from 'react-router';
import { Button, Card, Group, SimpleGrid, Stack, Text, TextInput, ThemeIcon } from '@mantine/core';
import { DatePickerInput } from '@mantine/dates';
import { IconArchive, IconCalendar, IconDatabaseExport, IconFileCertificate, IconFileSpreadsheet, IconFileTypePdf, IconFileZip } from '@tabler/icons-react';
import dayjs from 'dayjs';
import { downloadFile } from '@/api/client';
import { notifyError } from '@/api/hooks';
import { YearSelect } from '@/components/inputs';
import { Page, PageHeader } from '@/components/ui';

/** Text state remembered in localStorage (DATEV numbers rarely change). */
function useStoredState(key: string) {
  const [value, setValue] = useState(() => {
    try {
      return localStorage.getItem(key) ?? '';
    } catch {
      return '';
    }
  });
  const set = (v: string) => {
    setValue(v);
    try {
      localStorage.setItem(key, v);
    } catch {
      // storage unavailable – keep the value for this session only
    }
  };
  return [value, set] as const;
}

export function ExportsPage() {
  const navigate = useNavigate();
  const [range, setRange] = useState<[string | null, string | null]>([dayjs().startOf('year').format('YYYY-MM-DD'), dayjs().endOf('year').format('YYYY-MM-DD')]);
  const [advisor, setAdvisor] = useStoredState('sorobo.datev.advisor');
  const [client, setClient] = useStoredState('sorobo.datev.client');
  const [year, setYear] = useState(new Date().getFullYear());
  const [auditYear, setAuditYear] = useState(new Date().getFullYear() - 1);
  const [busy, setBusy] = useState<string | null>(null);
  const dl = async (key: string, path: string, params?: Record<string, string | number | undefined>) => {
    setBusy(key);
    try {
      await downloadFile(path, params);
    } catch (e) {
      notifyError(e, 'Export fehlgeschlagen');
    } finally {
      setBusy(null);
    }
  };
  const reports: { key: string; label: string; params?: Record<string, string | number> }[] = [
    { key: 'euer', label: 'Einnahmen-Überschuss-Rechnung' },
    { key: 'guv', label: 'Gewinn- und Verlustrechnung' },
    { key: 'bilanz', label: 'Bilanz' },
    { key: 'bwa', label: 'BWA (monatlich)' },
    { key: 'susa', label: 'Summen- und Saldenliste' },
    { key: 'journal', label: 'Buchungsjournal' },
    { key: 'open-items', label: 'Offene-Posten-Liste' },
    { key: 'assets', label: 'Anlagenverzeichnis' },
  ];
  return (
    <Page>
      <PageHeader title="Exporte & DATEV" subtitle="Daten für Steuerberatung, Finanzamt und Archiv" />
      <SimpleGrid cols={{ base: 1, lg: 2 }} spacing="lg">
        <Card>
          <Group mb="md">
            <ThemeIcon size={40} radius="md" variant="light">
              <IconFileSpreadsheet size={22} />
            </ThemeIcon>
            <div>
              <Text fw={650}>DATEV-Buchungsstapel</Text>
              <Text size="sm" c="dimmed">
                EXTF-Format (CSV) zum Import in DATEV Kanzlei-Rechnungswesen
              </Text>
            </div>
          </Group>
          <Stack gap="sm">
            <DatePickerInput type="range" label="Zeitraum" valueFormat="DD.MM.YYYY" value={range} onChange={(v) => setRange(v as [string | null, string | null])} leftSection={<IconCalendar size={16} />} />
            <SimpleGrid cols={2} spacing="sm">
              <TextInput label="Beraternummer" placeholder="z. B. 1001" value={advisor} onChange={(e) => setAdvisor(e.currentTarget.value)} />
              <TextInput label="Mandantennummer" placeholder="z. B. 1" value={client} onChange={(e) => setClient(e.currentTarget.value)} />
            </SimpleGrid>
            <Text size="xs" c="dimmed">
              Steuerbuchungen werden – soweit möglich – mit BU-Schlüssel bzw. über Automatikkonten übergeben. Bitte vor dem Import mit Ihrer Steuerberatung abstimmen.
            </Text>
            <Button leftSection={<IconDatabaseExport size={16} />} loading={busy === 'datev'} onClick={() => dl('datev', '/exports/datev', { from: range[0] ?? undefined, to: range[1] ?? undefined, advisor, client })} w="fit-content">
              DATEV-Export herunterladen
            </Button>
          </Stack>
        </Card>
        <Card>
          <Group mb="md">
            <ThemeIcon size={40} radius="md" variant="light" color="grape">
              <IconFileZip size={22} />
            </ThemeIcon>
            <div>
              <Text fw={650}>Belege als ZIP-Archiv</Text>
              <Text size="sm" c="dimmed">
                Alle hochgeladenen Belegdateien, benannt nach Datum und Belegnummer
              </Text>
            </div>
          </Group>
          <Stack gap="sm">
            <DatePickerInput type="range" label="Zeitraum" valueFormat="DD.MM.YYYY" value={range} onChange={(v) => setRange(v as [string | null, string | null])} leftSection={<IconCalendar size={16} />} />
            <Button variant="light" color="grape" leftSection={<IconFileZip size={16} />} loading={busy === 'zip'} onClick={() => dl('zip', '/exports/receipts', { from: range[0] ?? undefined, to: range[1] ?? undefined })} w="fit-content">
              Belege herunterladen
            </Button>
          </Stack>
        </Card>
        <Card>
          <Group justify="space-between" mb="md" wrap="nowrap">
            <Group wrap="nowrap">
              <ThemeIcon size={40} radius="md" variant="light" color="orange">
                <IconFileCertificate size={22} />
              </ThemeIcon>
              <div>
                <Text fw={650}>Datenexport für die Betriebsprüfung</Text>
                <Text size="sm" c="dimmed">
                  GoBD-Datenträgerüberlassung nach § 147 Abs. 6 AO
                </Text>
              </div>
            </Group>
            <YearSelect value={auditYear} onChange={setAuditYear} />
          </Group>
          <Stack gap="sm">
            <Text size="sm" c="dimmed">
              Buchungsjournal, Kontenplan, Kontakte, Rechnungen, Belege, Zahlungen, Anlagen und Änderungsprotokoll des Jahres als CSV-Dateien mit Beschreibung (index.xml) im Beschreibungsstandard der Finanzverwaltung – lesbar mit IDEA und anderen Prüfprogrammen.
            </Text>
            <Button variant="light" color="orange" leftSection={<IconFileCertificate size={16} />} loading={busy === 'gdpdu'} onClick={() => dl('gdpdu', '/exports/gdpdu', { year: auditYear })} w="fit-content">
              Datenexport herunterladen
            </Button>
          </Stack>
        </Card>
        <Card>
          <Group justify="space-between" mb="md">
            <Group>
              <ThemeIcon size={40} radius="md" variant="light" color="red">
                <IconFileTypePdf size={22} />
              </ThemeIcon>
              <div>
                <Text fw={650}>Berichte als PDF</Text>
                <Text size="sm" c="dimmed">
                  Jahresberichte für Ihre Unterlagen
                </Text>
              </div>
            </Group>
            <YearSelect value={year} onChange={setYear} />
          </Group>
          <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="xs">
            {reports.map((r) => (
              <Button key={r.key} variant="default" justify="space-between" rightSection={<IconFileTypePdf size={14} />} loading={busy === r.key} onClick={() => dl(r.key, `/reports/pdf/${r.key}`, { year })}>
                {r.label}
              </Button>
            ))}
          </SimpleGrid>
        </Card>
        <Card>
          <Group mb="md">
            <ThemeIcon size={40} radius="md" variant="light" color="teal">
              <IconArchive size={22} />
            </ThemeIcon>
            <div>
              <Text fw={650}>Datensicherung</Text>
              <Text size="sm" c="dimmed">
                Vollständige Sicherung aller Daten und Belege (ZIP)
              </Text>
            </div>
          </Group>
          <Group>
            <Button color="teal" leftSection={<IconArchive size={16} />} loading={busy === 'backup'} onClick={() => dl('backup', '/system/backup')}>
              Sicherung herunterladen
            </Button>
            <Button variant="subtle" onClick={() => navigate('/einstellungen/daten')}>
              Wiederherstellen …
            </Button>
          </Group>
          <Text size="xs" c="dimmed" mt="md">
            Hinweis: Nach GoBD sind Buchungsbelege und Rechnungen 8 bzw. 10 Jahre aufzubewahren. Bewahren Sie Sicherungen an einem zweiten Ort auf.
          </Text>
        </Card>
      </SimpleGrid>
    </Page>
  );
}
