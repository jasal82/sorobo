// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { useEffect, useMemo, useState } from 'react';
import { ActionIcon, Badge, Button, Card, Group, Modal, Progress, Select, SimpleGrid, Stack, Table, Tabs, Text, TextInput, Textarea, Tooltip } from '@mantine/core';
import { DatePickerInput } from '@mantine/dates';
import { IconCalendar, IconClock, IconEdit, IconFileExport, IconPlus, IconRestore, IconTrash, IconUsers } from '@tabler/icons-react';
import { api } from '@/api/client';
import { useAction, useApi } from '@/api/hooks';
import type { DocumentListItem, Person, PersonHours, TimeEntry, TimeEntryItem, TimeReport } from '@/api/types';
import { DateField, DecimalInput } from '@/components/inputs';
import { ListTable } from '@/components/ListTable';
import { Empty, Page, PageHeader, SectionCard, StatPill, confirmAction } from '@/components/ui';
import { exportCsv } from '@/lib/csv';
import { date, percent, qty, today } from '@/lib/format';
import { DOC_TYPES } from '@/lib/labels';

const hours = (h: number) => `${qty(h)} Std`;
const share = (h: number, total: number) => (total > 0 ? (h / total) * 100 : 0);

function documentLabel(number: string, title: string, contact: string) {
  return [number || 'Entwurf', title, contact].filter(Boolean).join(' · ');
}

/** Select options for the documents hours can be booked on, grouped by document type. */
function useDocumentOptions() {
  const { data } = useApi<DocumentListItem[]>('/documents');
  return useMemo(
    () =>
      Object.entries(DOC_TYPES)
        .map(([type, meta]) => ({
          group: meta.plural,
          items: (data ?? []).filter((d) => d.doc_type === type).map((d) => ({ value: String(d.id), label: documentLabel(d.number, d.title, d.contact_name) })),
        }))
        .filter((g) => g.items.length > 0),
    [data],
  );
}

function emptyEntry(personId: number, documentId: number): TimeEntry {
  return { id: 0, person_id: personId, document_id: documentId, date: today(), hours: 0, note: '', created_at: '', updated_at: '' };
}

function EntryModal({
  opened,
  onClose,
  entry,
  people,
  onSaved,
}: {
  opened: boolean;
  onClose: () => void;
  entry: TimeEntry | null;
  people: Person[];
  onSaved: (e: TimeEntry) => void;
}) {
  const documents = useDocumentOptions();
  const [form, setForm] = useState<TimeEntry>(emptyEntry(0, 0));
  useEffect(() => {
    if (opened) setForm(entry ?? emptyEntry(0, 0));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [opened, entry?.id]);
  const set = (patch: Partial<TimeEntry>) => setForm((f) => ({ ...f, ...patch }));
  const save = useAction((e: TimeEntry) => (e.id ? api.put<TimeEntry>(`/time-entries/${e.id}`, e) : api.post<TimeEntry>('/time-entries', e)), {
    success: 'Gespeichert',
    onSuccess: (saved) => {
      onSaved(saved);
      onClose();
    },
  });
  const personOptions = people.filter((p) => !p.archived || p.id === form.person_id).map((p) => ({ value: String(p.id), label: p.archived ? `${p.name} (archiviert)` : p.name }));
  const valid = form.person_id > 0 && form.document_id > 0 && form.hours > 0;
  return (
    <Modal opened={opened} onClose={onClose} title={<Text fw={650}>{form.id ? 'Eintrag bearbeiten' : 'Stunden erfassen'}</Text>}>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          if (valid) save.mutate(form);
        }}
      >
        <Stack gap="sm">
          <Select
            label="Person"
            required
            data={personOptions}
            value={form.person_id ? String(form.person_id) : null}
            onChange={(v) => set({ person_id: v ? Number(v) : 0 })}
            placeholder={personOptions.length === 0 ? 'Zuerst unter „Personen“ anlegen' : 'Person wählen'}
            data-autofocus={!form.person_id}
          />
          <Select
            label="Angebot / Dokument"
            required
            searchable
            data={documents}
            value={form.document_id ? String(form.document_id) : null}
            onChange={(v) => set({ document_id: v ? Number(v) : 0 })}
            placeholder="Dokument wählen"
            nothingFoundMessage="Kein Dokument gefunden"
          />
          <SimpleGrid cols={2} spacing="sm">
            <DateField label="Datum" required value={form.date} onChange={(v) => v && set({ date: v })} />
            <DecimalInput label="Stunden" required scale={2} placeholder="z. B. 1,5" value={form.hours || null} onChange={(h) => set({ hours: h })} data-autofocus={!!form.person_id} />
          </SimpleGrid>
          <Textarea label="Notiz" description="Was wurde gemacht? (optional)" minRows={2} value={form.note} onChange={(e) => set({ note: e.currentTarget.value })} inputWrapperOrder={['label', 'input', 'description', 'error']} />
          <Group justify="flex-end" mt="sm">
            <Button variant="default" onClick={onClose}>
              Abbrechen
            </Button>
            <Button type="submit" loading={save.isPending} disabled={!valid}>
              Speichern
            </Button>
          </Group>
        </Stack>
      </form>
    </Modal>
  );
}

function PeopleModal({ opened, onClose, people }: { opened: boolean; onClose: () => void; people: Person[] }) {
  const [name, setName] = useState('');
  const [editing, setEditing] = useState<{ id: number; name: string } | null>(null);
  const add = useAction((n: string) => api.post<Person>('/people', { name: n }), { success: 'Person angelegt', onSuccess: () => setName('') });
  const update = useAction((p: Person) => api.put<Person>(`/people/${p.id}`, p), { onSuccess: () => setEditing(null) });
  const remove = useAction((id: number) => api.del<{ message: string }>(`/people/${id}`), { success: (r) => (r.message === 'archived' ? 'Archiviert (es sind Stunden erfasst)' : 'Gelöscht') });
  return (
    <Modal opened={opened} onClose={onClose} title={<Text fw={650}>Personen</Text>}>
      <Stack gap="sm">
        <form
          onSubmit={(e) => {
            e.preventDefault();
            if (name.trim()) add.mutate(name.trim());
          }}
        >
          <Group gap="xs" align="flex-end" wrap="nowrap">
            <TextInput label="Neue Person" placeholder="Name" style={{ flex: 1 }} value={name} onChange={(e) => setName(e.currentTarget.value)} data-autofocus />
            <Button type="submit" leftSection={<IconPlus size={16} />} loading={add.isPending} disabled={!name.trim()}>
              Anlegen
            </Button>
          </Group>
        </form>
        {people.length === 0 ? (
          <Text size="sm" c="dimmed">
            Noch keine Personen angelegt.
          </Text>
        ) : (
          <Table verticalSpacing="xs">
            <Table.Tbody>
              {people.map((p) => (
                <Table.Tr key={p.id}>
                  <Table.Td>
                    {editing?.id === p.id ? (
                      <form
                        onSubmit={(e) => {
                          e.preventDefault();
                          update.mutate({ ...p, name: editing.name });
                        }}
                      >
                        <TextInput size="xs" value={editing.name} onChange={(e) => setEditing({ id: p.id, name: e.currentTarget.value })} onBlur={() => setEditing(null)} data-autofocus />
                      </form>
                    ) : (
                      <Group gap="xs">
                        <Text size="sm" c={p.archived ? 'dimmed' : undefined}>
                          {p.name}
                        </Text>
                        {p.archived && (
                          <Badge size="xs" variant="light" color="gray">
                            archiviert
                          </Badge>
                        )}
                      </Group>
                    )}
                  </Table.Td>
                  <Table.Td w={96}>
                    <Group gap={2} justify="flex-end" wrap="nowrap">
                      {p.archived ? (
                        <Tooltip label="Wieder aktivieren">
                          <ActionIcon aria-label="Wieder aktivieren" onClick={() => update.mutate({ ...p, archived: false })}>
                            <IconRestore size={16} />
                          </ActionIcon>
                        </Tooltip>
                      ) : (
                        <>
                          <Tooltip label="Umbenennen">
                            <ActionIcon aria-label="Umbenennen" onMouseDown={(e) => e.preventDefault()} onClick={() => setEditing({ id: p.id, name: p.name })}>
                              <IconEdit size={16} />
                            </ActionIcon>
                          </Tooltip>
                          <Tooltip label="Löschen">
                            <ActionIcon
                              color="red"
                              aria-label="Löschen"
                              onClick={() =>
                                confirmAction({
                                  title: 'Person löschen',
                                  message: `${p.name} wirklich löschen? Sind schon Stunden erfasst, wird die Person nur archiviert und bleibt in der Auswertung.`,
                                  confirmLabel: 'Löschen',
                                  danger: true,
                                  onConfirm: () => remove.mutate(p.id),
                                })
                              }
                            >
                              <IconTrash size={16} />
                            </ActionIcon>
                          </Tooltip>
                        </>
                      )}
                    </Group>
                  </Table.Td>
                </Table.Tr>
              ))}
            </Table.Tbody>
          </Table>
        )}
      </Stack>
    </Modal>
  );
}

function PeopleTable({ list, total }: { list: PersonHours[]; total: number }) {
  return (
    <Table verticalSpacing="xs">
      <Table.Thead>
        <Table.Tr>
          <Table.Th>Person</Table.Th>
          <Table.Th ta="right">Stunden</Table.Th>
          <Table.Th w="40%">Anteil</Table.Th>
        </Table.Tr>
      </Table.Thead>
      <Table.Tbody>
        {list.map((p) => (
          <Table.Tr key={p.person_id}>
            <Table.Td>{p.name}</Table.Td>
            <Table.Td ta="right" className="tabular">
              {hours(p.hours)}
            </Table.Td>
            <Table.Td>
              <Group gap="xs" wrap="nowrap">
                <Progress value={share(p.hours, total)} flex={1} size="sm" aria-label={`Anteil ${p.name}`} />
                <Text size="sm" className="tabular" w={64} ta="right">
                  {percent(share(p.hours, total))}
                </Text>
              </Group>
            </Table.Td>
          </Table.Tr>
        ))}
      </Table.Tbody>
    </Table>
  );
}

function ReportTab({ report }: { report: TimeReport | undefined }) {
  if (!report || report.documents.length === 0) {
    return <Empty icon={<IconClock size={22} />} title="Keine Stunden im gewählten Zeitraum" description="Erfassen Sie Stunden, um die Auswertung zu sehen." />;
  }
  return (
    <Stack gap="md">
      <SectionCard title="Stunden je Person" description="Über alle Angebote und Dokumente der Auswahl – die Grundlage für die Verteilung des Gewinns">
        <PeopleTable list={report.people} total={report.total} />
      </SectionCard>
      {report.documents.map((d) => (
        <SectionCard
          key={d.document_id}
          title={documentLabel(d.number, d.title, d.contact_name)}
          actions={
            <Text fw={650} className="tabular">
              {hours(d.total)}
            </Text>
          }
        >
          <PeopleTable list={d.people} total={d.total} />
        </SectionCard>
      ))}
    </Stack>
  );
}

export function TimePage() {
  const { data: people } = useApi<Person[]>('/people');
  const documents = useDocumentOptions();
  const [range, setRange] = useState<[string | null, string | null]>([null, null]);
  const [personId, setPersonId] = useState<string | null>(null);
  const [documentId, setDocumentId] = useState<string | null>(null);
  const [tab, setTab] = useState<string | null>('entries');
  const [editing, setEditing] = useState<TimeEntry | null>(null);
  const [entryOpen, setEntryOpen] = useState(false);
  const [peopleOpen, setPeopleOpen] = useState(false);
  // The next entry starts with what was booked last: usually one person books several entries.
  const [last, setLast] = useState<{ person: number; document: number }>({ person: 0, document: 0 });

  const params = { from: range[0] ?? undefined, to: range[1] ?? undefined, person_id: personId ?? undefined, document_id: documentId ?? undefined };
  const { data: entries, isFetching } = useApi<TimeEntryItem[]>('/time-entries', params);
  const { data: report } = useApi<TimeReport>('/time-entries/report', params);
  const remove = useAction((id: number) => api.del(`/time-entries/${id}`), { success: 'Eintrag gelöscht' });

  const allPeople = people ?? [];
  const activePeople = allPeople.filter((p) => !p.archived);
  const openNew = () => {
    setEditing({ ...emptyEntry(last.person || (activePeople.length === 1 ? activePeople[0].id : 0), last.document) });
    setEntryOpen(true);
  };
  const openEdit = (e: TimeEntry) => {
    setEditing(e);
    setEntryOpen(true);
  };

  return (
    <Page>
      <PageHeader
        title="Zeiterfassung"
        subtitle="Wer hat wie viele Stunden an welchem Angebot gearbeitet?"
        actions={
          <>
            <Button variant="default" leftSection={<IconUsers size={16} />} onClick={() => setPeopleOpen(true)}>
              Personen
            </Button>
            <Button
              variant="default"
              leftSection={<IconFileExport size={16} />}
              disabled={!entries?.length}
              onClick={() =>
                exportCsv('Zeiterfassung', entries ?? [], [
                  { label: 'Datum', value: (e) => e.entry.date },
                  { label: 'Person', value: (e) => e.person_name },
                  { label: 'Dokument', value: (e) => e.document_number },
                  { label: 'Titel', value: (e) => e.document_title },
                  { label: 'Kunde', value: (e) => e.contact_name },
                  { label: 'Stunden', value: (e) => e.entry.hours },
                  { label: 'Notiz', value: (e) => e.entry.note },
                ])
              }
            >
              Export
            </Button>
            <Button leftSection={<IconPlus size={16} />} onClick={openNew}>
              Stunden erfassen
            </Button>
          </>
        }
      />
      <Stack gap="md">
        <Group gap="sm" align="flex-end">
          <DatePickerInput type="range" placeholder="Zeitraum" valueFormat="DD.MM.YYYY" value={range} onChange={(v) => setRange(v as [string | null, string | null])} clearable leftSection={<IconCalendar size={16} />} w={240} />
          <Select
            placeholder="Alle Personen"
            clearable
            data={allPeople.map((p) => ({ value: String(p.id), label: p.archived ? `${p.name} (archiviert)` : p.name }))}
            value={personId}
            onChange={setPersonId}
            w={200}
          />
          <Select placeholder="Alle Angebote / Dokumente" clearable searchable data={documents} value={documentId} onChange={setDocumentId} w={320} />
          <Group gap="sm" ml="auto">
            <StatPill label="Stunden gesamt" value={hours(report?.total ?? 0)} />
          </Group>
        </Group>
        <Tabs value={tab} onChange={setTab} keepMounted={false}>
          <Tabs.List mb="md">
            <Tabs.Tab value="entries">Einträge</Tabs.Tab>
            <Tabs.Tab value="report">Auswertung</Tabs.Tab>
          </Tabs.List>
          <Tabs.Panel value="entries">
            <Card padding={0}>
              {!isFetching && (entries ?? []).length === 0 && !range[0] && !personId && !documentId ? (
                <Empty
                  icon={<IconClock size={22} />}
                  title="Noch keine Stunden erfasst"
                  description="Erfassen Sie Stunden je Person und Angebot, um am Ende die Anteile zu sehen."
                  action={
                    <Button leftSection={<IconPlus size={16} />} onClick={openNew}>
                      Stunden erfassen
                    </Button>
                  }
                />
              ) : (
                <ListTable<TimeEntryItem>
                  records={entries ?? []}
                  idAccessor={(r) => r.entry.id}
                  fetching={isFetching}
                  defaultSort={{ column: 'date', direction: 'desc' }}
                  onRowClick={(r) => openEdit(r.entry)}
                  columns={[
                    { accessor: 'date', title: 'Datum', sortable: true, width: 110, render: (r) => date(r.entry.date) },
                    { accessor: 'person_name', title: 'Person', sortable: true },
                    {
                      accessor: 'document_number',
                      title: 'Angebot / Dokument',
                      render: (r) => (
                        <Text size="sm" lineClamp={1}>
                          {documentLabel(r.document_number, r.document_title, r.contact_name)}
                        </Text>
                      ),
                    },
                    { accessor: 'note', title: 'Notiz', visibleMediaQuery: '(min-width: 62em)', ellipsis: true, render: (r) => r.entry.note },
                    {
                      accessor: 'hours',
                      title: 'Stunden',
                      textAlign: 'right',
                      width: 110,
                      render: (r) => (
                        <Text size="sm" fw={600} className="tabular">
                          {qty(r.entry.hours)}
                        </Text>
                      ),
                    },
                    {
                      accessor: 'actions',
                      title: '',
                      width: 60,
                      textAlign: 'right',
                      render: (r) => (
                        <Group justify="flex-end" wrap="nowrap" onClick={(e) => e.stopPropagation()}>
                          <Tooltip label="Löschen">
                            <ActionIcon
                              color="red"
                              aria-label="Löschen"
                              onClick={() =>
                                confirmAction({
                                  title: 'Eintrag löschen',
                                  message: `${qty(r.entry.hours)} Std von ${r.person_name} am ${date(r.entry.date)} wirklich löschen?`,
                                  confirmLabel: 'Löschen',
                                  danger: true,
                                  onConfirm: () => remove.mutate(r.entry.id),
                                })
                              }
                            >
                              <IconTrash size={16} />
                            </ActionIcon>
                          </Tooltip>
                        </Group>
                      ),
                    },
                  ]}
                />
              )}
            </Card>
          </Tabs.Panel>
          <Tabs.Panel value="report">
            <ReportTab report={report} />
          </Tabs.Panel>
        </Tabs>
      </Stack>
      <EntryModal opened={entryOpen} onClose={() => setEntryOpen(false)} entry={editing} people={allPeople} onSaved={(e) => setLast({ person: e.person_id, document: e.document_id })} />
      <PeopleModal opened={peopleOpen} onClose={() => setPeopleOpen(false)} people={allPeople} />
    </Page>
  );
}
