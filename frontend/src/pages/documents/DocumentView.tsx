// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { useEffect, useState } from 'react';
import { Link, useNavigate, useParams } from 'react-router';
import {
  ActionIcon,
  Alert,
  Anchor,
  Badge,
  Box,
  Button,
  Card,
  Center,
  Divider,
  Grid,
  Group,
  List,
  Loader,
  Menu,
  Modal,
  TextInput,
  Textarea,
  NumberInput,
  SegmentedControl,
  Stack,
  Text,
  ThemeIcon,
  Timeline,
  Tooltip,
} from '@mantine/core';
import {
  IconAlertTriangle,
  IconArrowRight,
  IconBan,
  IconChevronDown,
  IconCheck,
  IconCircleCheck,
  IconClipboardCheck,
  IconCopy,
  IconCash,
  IconDots,
  IconDownload,
  IconEdit,
  IconExternalLink,
  IconFileCode,
  IconFileInvoice,
  IconHistory,
  IconMail,
  IconPercentage,
  IconPrinter,
  IconReceiptRefund,
  IconTrash,
  IconTruckDelivery,
  IconX,
  IconBellRinging,
  IconFlag,
  IconRepeat,
  IconPaperclip,
  IconSend,
} from '@tabler/icons-react';
import { api, downloadFile, openFile } from '@/api/client';
import { notifyError, useAction, useApi, useSettings } from '@/api/hooks';
import type { DocumentFull, EinvoiceCheck, EmailDraft, EmailRequest, IdResponse, MandateView, Pages, RecurringView } from '@/api/types';
import { PaymentModal } from '@/components/PaymentModal';
import { PagesPreview } from '@/components/DocumentParts';
import { CYCLES, RecurringError, RecurringModal, recurringMode } from '@/components/Recurring';
import { InfoRow, Money, Page, PageHeader, SectionCard, StatusBadge, confirmAction } from '@/components/ui';
import { documentIncoming, documentRemaining } from '@/lib/calc';
import { date, dateTime, formatIban, money, relativeDays } from '@/lib/format';
import { isDesktop } from '@/lib/desktop';
import { DOC_TYPES, docLabel, type DocType } from '@/lib/labels';

function PartialModal({ opened, onClose, doc }: { opened: boolean; onClose: () => void; doc: DocumentFull }) {
  const navigate = useNavigate();
  const [mode, setMode] = useState('percent');
  const [value, setValue] = useState<number>(30);
  const already = doc.related.filter((r) => r.relation === 'partial' && r.status !== 'cancelled').reduce((s, r) => s + r.net_total, 0);
  const create = useAction((v: { mode: string; value: number }) => api.post<IdResponse>(`/documents/${doc.doc.id}/partial`, v), {
    success: 'Abschlagsrechnung als Entwurf erstellt',
    onSuccess: (r) => navigate(`/dokumente/${r.id}/bearbeiten`),
  });
  return (
    <Modal opened={opened} onClose={onClose} title={<Text fw={650}>Abschlagsrechnung erstellen</Text>}>
      <Stack>
        <Text size="sm" c="dimmed">
          Auftragswert: <b>{money(doc.totals.items_net)}</b> netto · bereits abgerechnet: <b>{money(already)}</b> netto
        </Text>
        <SegmentedControl
          value={mode}
          onChange={(m) => {
            setMode(m);
            setValue(m === 'percent' ? 30 : Math.round(doc.totals.items_net * 0.3));
          }}
          data={[
            { value: 'percent', label: 'Prozent des Auftrags' },
            { value: 'amount', label: 'Fester Betrag (netto)' },
          ]}
        />
        <NumberInput
          label={mode === 'percent' ? 'Abschlag in %' : 'Abschlag netto'}
          value={value}
          onChange={(v) => setValue(Number(v) || 0)}
          decimalScale={2}
          decimalSeparator=","
          thousandSeparator="."
          suffix={mode === 'percent' ? ' %' : ' €'}
          min={0}
        />
        <Text size="sm">
          Abschlag netto: <b>{money(mode === 'percent' ? (doc.totals.items_net * value) / 100 : value)}</b>
        </Text>
        <Group justify="flex-end">
          <Button variant="default" onClick={onClose}>
            Abbrechen
          </Button>
          <Button onClick={() => create.mutate({ mode, value })} loading={create.isPending}>
            Entwurf erstellen
          </Button>
        </Group>
      </Stack>
    </Modal>
  );
}

function DunningModal({ opened, onClose, doc }: { opened: boolean; onClose: () => void; doc: DocumentFull }) {
  // Levels, names and fees as configured in Einstellungen → Mahnwesen.
  const { data: settings } = useSettings();
  const levels = [...(settings?.dunning.levels ?? [])].sort((a, b) => a.level - b.level);
  const maxLevel = levels.length ? levels[levels.length - 1].level : 0;
  const nextLevel = doc.dunnings.length ? Math.max(...doc.dunnings.map((d) => d.level)) + 1 : 0;
  const [level, setLevel] = useState('0');
  const [pages, setPages] = useState<string[] | undefined>();
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (opened && levels.length) setLevel(String(Math.min(nextLevel, maxLevel)));
    // Only when the dialog opens or the configuration arrives.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [opened, levels.length]);
  useEffect(() => {
    if (!opened) return;
    setPages(undefined);
    setError(null);
    api
      .post<Pages>('/dunning/preview', { document_id: doc.doc.id, level: Number(level) })
      .then((r) => setPages(r.pages))
      .catch((e) => setError(e.message));
  }, [opened, level, doc.doc.id]);
  const create = useAction((lvl: number) => api.post<{ id: number }>('/dunning', { document_id: doc.doc.id, level: lvl }), {
    success: 'Mahnung erstellt',
    onSuccess: (r) => {
      onClose();
      openFile(`/dunning/${r.id}/pdf`);
    },
  });
  const current = levels.find((l) => String(l.level) === level);
  return (
    <Modal opened={opened} onClose={onClose} size={900} title={<Text fw={650}>Mahnung erstellen – {doc.doc.number}</Text>}>
      <Grid>
        <Grid.Col span={{ base: 12, md: 4 }}>
          <Stack>
            <SegmentedControl orientation="vertical" fullWidth value={level} onChange={setLevel} data={levels.map((l) => ({ value: String(l.level), label: l.name || l.title }))} />
            <Text size="sm" c="dimmed">
              Offener Betrag {money(doc.open_amount)} · {doc.days_overdue} Tage überfällig{current && current.fee > 0 ? ` · Gebühr ${money(current.fee)}` : ''}. Gebühren und Texte richten sich nach den Mahnstufen in den Einstellungen.
            </Text>
            <Button leftSection={<IconBellRinging size={16} />} onClick={() => create.mutate(Number(level))} loading={create.isPending} disabled={!current}>
              {current ? `${current.name || current.title} erstellen` : 'Mahnung erstellen'}
            </Button>
          </Stack>
        </Grid.Col>
        <Grid.Col span={{ base: 12, md: 8 }}>
          <Box mah={620} style={{ overflowY: 'auto' }} bg="var(--app-bg)" p="sm" bdrs="md">
            <PagesPreview pages={pages} error={error} />
          </Box>
        </Grid.Col>
      </Grid>
    </Modal>
  );
}

function EinvoiceMenu({ doc }: { doc: DocumentFull }) {
  const [check, setCheck] = useState<{ format: string; result: EinvoiceCheck } | null>(null);
  const run = async (format: 'xrechnung' | 'zugferd') => {
    try {
      const result = await api.get<EinvoiceCheck>(`/documents/${doc.doc.id}/einvoice-check`, { format });
      if (result.errors.length || result.warnings.length) {
        setCheck({ format, result });
        if (result.errors.length) return;
      }
      await downloadFile(`/documents/${doc.doc.id}/${format}`);
    } catch (e) {
      notifyError(e, 'E-Rechnung konnte nicht erstellt werden');
    }
  };
  return (
    <>
      <Menu position="bottom-end" width={280}>
        <Menu.Target>
          <Button variant="default" leftSection={<IconFileCode size={16} />}>
            E-Rechnung
          </Button>
        </Menu.Target>
        <Menu.Dropdown>
          <Menu.Label>Nach EN 16931</Menu.Label>
          <Menu.Item leftSection={<IconFileInvoice size={16} />} onClick={() => run('zugferd')}>
            <Text size="sm">ZUGFeRD-PDF herunterladen</Text>
            <Text size="xs" c="dimmed">
              PDF/A-3 mit eingebetteter XML (Profil EN 16931)
            </Text>
          </Menu.Item>
          <Menu.Item leftSection={<IconFileCode size={16} />} onClick={() => run('xrechnung')}>
            <Text size="sm">XRechnung (XML) herunterladen</Text>
            <Text size="xs" c="dimmed">
              XRechnung 3.0, Syntax CII – z. B. für Behörden
            </Text>
          </Menu.Item>
        </Menu.Dropdown>
      </Menu>
      <Modal opened={!!check} onClose={() => setCheck(null)} title={<Text fw={650}>Prüfung der E-Rechnung</Text>}>
        {check && (
          <Stack>
            {check.result.errors.length > 0 && (
              <Alert color="red" icon={<IconX size={16} />} title="Bitte ergänzen Sie folgende Angaben">
                <List size="sm">
                  {check.result.errors.map((e) => (
                    <List.Item key={e}>{e}</List.Item>
                  ))}
                </List>
              </Alert>
            )}
            {check.result.warnings.length > 0 && (
              <Alert color="yellow" icon={<IconAlertTriangle size={16} />} title="Hinweise">
                <List size="sm">
                  {check.result.warnings.map((e) => (
                    <List.Item key={e}>{e}</List.Item>
                  ))}
                </List>
              </Alert>
            )}
            <Group justify="flex-end">
              {check.result.errors.length > 0 && (
                <Button component={Link} to="/einstellungen/unternehmen" variant="default">
                  Zu den Unternehmensdaten
                </Button>
              )}
              <Button onClick={() => setCheck(null)}>OK</Button>
            </Group>
          </Stack>
        )}
      </Modal>
    </>
  );
}

/** Sends a document from Sorobo (SMTP settings under Einstellungen → E-Mail). */
function SendEmailModal({ opened, onClose, doc }: { opened: boolean; onClose: () => void; doc: DocumentFull }) {
  const [mail, setMail] = useState<EmailRequest>({ to: '', cc: '', subject: '', body: '' });
  const [attachments, setAttachments] = useState<string[]>([]);
  useEffect(() => {
    if (!opened) return;
    api
      .get<EmailDraft>(`/documents/${doc.doc.id}/email`)
      .then((d) => {
        setMail({ to: d.to, cc: '', subject: d.subject, body: d.body });
        setAttachments(d.attachments);
      })
      .catch((e) => notifyError(e));
  }, [opened, doc.doc.id]);
  const send = useAction((m: EmailRequest) => api.post(`/documents/${doc.doc.id}/email`, m), { success: 'E-Mail gesendet', onSuccess: onClose, errorTitle: 'E-Mail nicht gesendet' });
  const set = (p: Partial<EmailRequest>) => setMail((m) => ({ ...m, ...p }));
  return (
    <Modal opened={opened} onClose={onClose} size="lg" title={<Text fw={650}>Per E-Mail senden</Text>}>
      <Stack gap="sm">
        <TextInput label="An" required value={mail.to} onChange={(e) => set({ to: e.currentTarget.value })} description="Mehrere Adressen mit Komma trennen" />
        <TextInput label="Kopie (CC)" value={mail.cc} onChange={(e) => set({ cc: e.currentTarget.value })} />
        <TextInput label="Betreff" value={mail.subject} onChange={(e) => set({ subject: e.currentTarget.value })} />
        <Textarea label="Text" autosize minRows={8} value={mail.body} onChange={(e) => set({ body: e.currentTarget.value })} />
        <Group gap={6}>
          {attachments.map((a) => (
            <Badge key={a} variant="light" color="gray" leftSection={<IconPaperclip size={12} />} tt="none">
              {a}
            </Badge>
          ))}
        </Group>
        <Group justify="flex-end">
          <Button variant="default" onClick={onClose}>
            Abbrechen
          </Button>
          <Button leftSection={<IconSend size={16} />} onClick={() => send.mutate(mail)} loading={send.isPending} disabled={!mail.to.trim()}>
            Senden
          </Button>
        </Group>
      </Stack>
    </Modal>
  );
}

const ACTIVITY_COLORS: Record<string, string> = {
  created: 'gray',
  finalized: 'brand',
  payment: 'teal',
  sent: 'blue',
  cancelled: 'red',
  dunning: 'orange',
  writeoff: 'orange',
};

export function DocumentViewPage() {
  const { id } = useParams();
  const navigate = useNavigate();
  const { data, isLoading, error } = useApi<DocumentFull>(`/documents/${id}`);
  const { data: preview, isFetching: previewLoading, error: previewError } = useApi<Pages>(`/documents/${id}/preview`, undefined, { keepPrevious: true });
  const [payOpen, setPayOpen] = useState(false);
  const [partialOpen, setPartialOpen] = useState(false);
  const [dunningOpen, setDunningOpen] = useState(false);
  const [recurringOpen, setRecurringOpen] = useState(false);
  const [sendOpen, setSendOpen] = useState(false);
  const { data: settings } = useSettings();
  const { data: recurringList } = useApi<RecurringView[]>('/recurring');
  const recurring = recurringList?.find((r) => r.recurring.template_id === Number(id))?.recurring ?? null;
  const { data: mandates } = useApi<MandateView[]>(data?.doc.payment_method === 'debit' ? '/mandates' : null, { contact_id: data?.doc.contact_id ?? undefined });
  const mandate = mandates?.find((m) => m.mandate.id === data?.doc.mandate_id);

  useEffect(() => {
    if (data && data.doc.status === 'draft') navigate(`/dokumente/${data.doc.id}/bearbeiten`, { replace: true });
  }, [data, navigate]);

  const goTo = (r: IdResponse) => navigate(`/dokumente/${r.id}${r.id ? '' : ''}`);
  const status = useAction((s: string) => api.post(`/documents/${id}/status`, { status: s }), { success: 'Status geändert' });
  const sent = useAction((via: string) => api.post(`/documents/${id}/sent`, { via }), { success: 'Als versendet markiert' });
  const copy = useAction(() => api.post<IdResponse>(`/documents/${id}/copy`), { success: 'Kopie erstellt', onSuccess: (r) => navigate(`/dokumente/${r.id}/bearbeiten`) });
  const convert = useAction((target: string) => api.post<IdResponse>(`/documents/${id}/convert`, { target }), {
    success: (_r, t) => `${DOC_TYPES[t as DocType].singular} als Entwurf erstellt`,
    onSuccess: (r) => navigate(`/dokumente/${r.id}/bearbeiten`),
  });
  const cancel = useAction(() => api.post<IdResponse>(`/documents/${id}/cancel`), { success: 'Rechnung storniert', onSuccess: goTo });
  const creditNote = useAction(() => api.post<IdResponse>(`/documents/${id}/credit-note`), { success: 'Gutschrift als Entwurf erstellt', onSuccess: (r) => navigate(`/dokumente/${r.id}/bearbeiten`) });
  const finalInvoice = useAction(() => api.post<IdResponse>(`/documents/${id}/final`), { success: 'Schlussrechnung als Entwurf erstellt', onSuccess: (r) => navigate(`/dokumente/${r.id}/bearbeiten`) });
  const remove = useAction(() => api.del(`/documents/${id}`), { success: 'Dokument gelöscht', onSuccess: () => navigate(-1) });
  const removePayment = useAction((allocId: number) => api.del(`/allocations/${allocId}`), { success: 'Zahlung entfernt' });
  const removeDunning = useAction((dId: number) => api.del(`/dunning/${dId}`), { success: 'Mahnung gelöscht' });

  if (isLoading || !data) {
    return (
      <Center h="60vh">{error ? <Text c="red">{error.message}</Text> : <Loader />}</Center>
    );
  }
  const { doc, contact } = data;
  const meta = DOC_TYPES[doc.doc_type as DocType];
  const label = docLabel(doc.doc_type, doc.kind);
  const isInvoice = doc.doc_type === 'invoice';
  const isInvoiceLike = isInvoice || doc.doc_type === 'credit_note';
  // Credit notes and negative invoices (Schlussrechnung below its Abschläge) are refunded.
  const remaining = documentRemaining(doc, data.open_amount);
  const refund = !documentIncoming(doc);
  const canPay = isInvoiceLike && remaining > 0.005 && doc.status !== 'cancelled';
  const canPartial = (doc.doc_type === 'quote' || doc.doc_type === 'order') && doc.status !== 'rejected';
  const hasFinal = data.related.some((r) => r.relation === 'final' && r.status !== 'cancelled');
  const hasPartials = data.related.some((r) => r.relation === 'partial' && r.status !== 'cancelled' && r.status !== 'draft');

  // Converting twice is usually a mistake, e.g. a quote that was already invoiced.
  const convertTo = (target: DocType) => {
    const existing = data.related.filter((r) => r.relation === 'derived' && r.doc_type === target && r.status !== 'cancelled');
    if (existing.length === 0) {
      convert.mutate(target);
      return;
    }
    const names = existing.map((r) => `${docLabel(r.doc_type, r.kind)} ${r.number || `(Entwurf vom ${date(r.date)})`}`).join(', ');
    confirmAction({
      title: 'Bereits umgewandelt',
      message: `Aus diesem Dokument wurde bereits erstellt: ${names}. Trotzdem erneut umwandeln?`,
      confirmLabel: 'Erneut umwandeln',
      onConfirm: () => convert.mutate(target),
    });
  };

  const mail = async () => {
    if (settings?.email.enabled) {
      setSendOpen(true);
      return;
    }
    // Without SMTP: save the attachments, open the mail program with the template text and only
    // mark the document as sent once the user confirms it.
    try {
      const d = await api.get<EmailDraft>(`/documents/${id}/email`);
      await downloadFile(`/documents/${id}/pdf`);
      if (isInvoiceLike && doc.einvoice_format === 'xrechnung') await downloadFile(`/documents/${id}/xrechnung`);
      window.location.href = `mailto:${encodeURIComponent(d.to)}?subject=${encodeURIComponent(d.subject)}&body=${encodeURIComponent(d.body)}`;
      confirmAction({
        title: 'E-Mail versendet?',
        message: `Bitte ${d.attachments.join(' und ')} in der E-Mail anhängen. Wurde die E-Mail an ${d.to || 'den Empfänger'} gesendet? Dann wird ${label} ${doc.number} als versendet markiert.`,
        confirmLabel: 'Als versendet markieren',
        onConfirm: () => sent.mutate('E-Mail'),
      });
    } catch (e) {
      notifyError(e);
    }
  };

  return (
    <Page size="full">
      <PageHeader
        back={{ to: meta?.route ?? '/', label: meta?.plural ?? 'Zurück' }}
        title={`${label} ${doc.number}`}
        badge={<StatusBadge status={doc.status} docType={doc.doc_type} daysOverdue={data.days_overdue} size="lg" />}
        subtitle={
          <>
            {contact ? (
              <Anchor component={Link} to={`/kontakte/${contact.id}`} size="sm">
                {doc.address.split('\n')[0]}
              </Anchor>
            ) : (
              doc.address.split('\n')[0]
            )}{' '}
            · {date(doc.date)} · {money(doc.gross_total)}
          </>
        }
        actions={
          <>
            {canPay && (
              <Button leftSection={<IconCash size={16} />} onClick={() => setPayOpen(true)} color="teal">
                {refund ? 'Erstattung erfassen' : 'Zahlung erfassen'}
              </Button>
            )}
            {!isInvoiceLike && (
              <Menu position="bottom-end" width={270}>
                <Menu.Target>
                  <Button leftSection={<IconArrowRight size={16} />} rightSection={<IconChevronDown size={14} />} loading={convert.isPending || finalInvoice.isPending}>
                    Weiterführen
                  </Button>
                </Menu.Target>
                <Menu.Dropdown>
                {doc.doc_type === 'quote' && (
                  <Menu.Item leftSection={<IconClipboardCheck size={16} />} onClick={() => convertTo('order')}>
                    In Auftragsbestätigung umwandeln
                  </Menu.Item>
                )}
                {doc.doc_type !== 'delivery' && (
                  <Menu.Item leftSection={<IconTruckDelivery size={16} />} onClick={() => convertTo('delivery')}>
                    Lieferschein erstellen
                  </Menu.Item>
                )}
                {!hasPartials && !hasFinal && (
                  <Menu.Item leftSection={<IconFileInvoice size={16} />} onClick={() => convertTo('invoice')}>
                    In Rechnung umwandeln
                  </Menu.Item>
                )}
                {canPartial && (
                  <Menu.Item leftSection={<IconPercentage size={16} />} onClick={() => setPartialOpen(true)} disabled={hasFinal}>
                    Abschlagsrechnung erstellen
                  </Menu.Item>
                )}
                {canPartial && hasPartials && !hasFinal && (
                  <Menu.Item leftSection={<IconFlag size={16} />} onClick={() => finalInvoice.mutate()}>
                    Schlussrechnung erstellen
                  </Menu.Item>
                )}
                </Menu.Dropdown>
              </Menu>
            )}
            <Button variant="default" leftSection={<IconDownload size={16} />} onClick={() => downloadFile(`/documents/${id}/pdf`).catch((e) => notifyError(e))}>
              PDF
            </Button>
            <Tooltip label={isDesktop ? 'In eigenem Fenster öffnen / drucken' : 'In neuem Tab öffnen / drucken'}>
              <ActionIcon variant="default" size="lg" onClick={() => openFile(`/documents/${id}/pdf`)} aria-label="Drucken">
                <IconPrinter size={18} />
              </ActionIcon>
            </Tooltip>
            <Button variant="default" leftSection={<IconMail size={16} />} onClick={mail}>
              Senden
            </Button>
            {isInvoiceLike && <EinvoiceMenu doc={data} />}
            <Menu position="bottom-end" width={270}>
              <Menu.Target>
                <ActionIcon variant="default" size="lg" aria-label="Weitere Aktionen">
                  <IconDots size={18} />
                </ActionIcon>
              </Menu.Target>
              <Menu.Dropdown>
                {!isInvoiceLike && (
                  <Menu.Item leftSection={<IconEdit size={16} />} onClick={() => navigate(`/dokumente/${id}/bearbeiten`)}>
                    Bearbeiten
                  </Menu.Item>
                )}
                <Menu.Item leftSection={<IconCopy size={16} />} onClick={() => copy.mutate()}>
                  Kopieren
                </Menu.Item>
                {doc.doc_type === 'quote' && (
                  <>
                    <Menu.Divider />
                    <Menu.Label>Angebot</Menu.Label>
                    {doc.status !== 'accepted' && (
                      <Menu.Item leftSection={<IconCheck size={16} />} color="teal" onClick={() => status.mutate('accepted')}>
                        Als angenommen markieren
                      </Menu.Item>
                    )}
                    {doc.status !== 'rejected' && (
                      <Menu.Item leftSection={<IconX size={16} />} onClick={() => status.mutate('rejected')}>
                        Als abgelehnt markieren
                      </Menu.Item>
                    )}
                    {doc.status !== 'open' && <Menu.Item onClick={() => status.mutate('open')}>Wieder öffnen</Menu.Item>}
                  </>
                )}
                {(doc.doc_type === 'order' || doc.doc_type === 'delivery') && (
                  <Menu.Item leftSection={<IconCircleCheck size={16} />} onClick={() => status.mutate(doc.status === 'done' ? 'open' : 'done')}>
                    {doc.status === 'done' ? 'Wieder öffnen' : doc.doc_type === 'delivery' ? 'Als geliefert markieren' : 'Als erledigt markieren'}
                  </Menu.Item>
                )}
                {isInvoice && (
                  <>
                    <Menu.Divider />
                    {data.open_amount > 0 && data.days_overdue > 0 && (
                      <Menu.Item leftSection={<IconBellRinging size={16} />} color="orange" onClick={() => setDunningOpen(true)}>
                        Mahnung / Zahlungserinnerung
                      </Menu.Item>
                    )}
                    {!doc.sent_at && (
                      <Menu.Item leftSection={<IconMail size={16} />} onClick={() => sent.mutate('Post')}>
                        Als versendet markieren
                      </Menu.Item>
                    )}
                    {doc.kind === 'standard' && doc.status !== 'cancelled' && (
                      <Menu.Item leftSection={<IconRepeat size={16} />} onClick={() => setRecurringOpen(true)}>
                        {recurring ? 'Abo-Rechnung bearbeiten …' : 'Als Abo-Rechnung wiederholen …'}
                      </Menu.Item>
                    )}
                    {doc.kind !== 'cancellation' && doc.status !== 'cancelled' && (
                      <Menu.Item leftSection={<IconReceiptRefund size={16} />} onClick={() => creditNote.mutate()}>
                        Gutschrift / Rechnungskorrektur
                      </Menu.Item>
                    )}
                    {doc.kind !== 'cancellation' && doc.status !== 'cancelled' && (
                      <Menu.Item
                        leftSection={<IconBan size={16} />}
                        color="red"
                        onClick={() =>
                          confirmAction({
                            title: 'Rechnung stornieren',
                            message: `Zur Rechnung ${doc.number} wird eine Stornorechnung erstellt. Die Rechnung gilt danach als storniert. Dieser Vorgang kann nicht rückgängig gemacht werden.`,
                            confirmLabel: 'Stornieren',
                            danger: true,
                            onConfirm: () => cancel.mutate(),
                          })
                        }
                      >
                        Stornieren
                      </Menu.Item>
                    )}
                  </>
                )}
                {!isInvoiceLike && (
                  <>
                    <Menu.Divider />
                    <Menu.Item
                      leftSection={<IconTrash size={16} />}
                      color="red"
                      onClick={() => confirmAction({ title: 'Dokument löschen', message: `${label} ${doc.number} wirklich löschen?`, confirmLabel: 'Löschen', danger: true, onConfirm: () => remove.mutate() })}
                    >
                      Löschen
                    </Menu.Item>
                  </>
                )}
              </Menu.Dropdown>
            </Menu>
          </>
        }
      />

      <Grid gap="lg">
        <Grid.Col span={{ base: 12, lg: 7, xl: 8 }}>
          <Box bg="var(--app-bg)">
            <PagesPreview pages={preview?.pages} loading={previewLoading} error={previewError ? previewError.message : null} maxWidth={860} />
          </Box>
        </Grid.Col>
        <Grid.Col span={{ base: 12, lg: 5, xl: 4 }}>
          <Stack gap="lg">
            {doc.status === 'cancelled' && (
              <Alert color="gray" icon={<IconBan size={16} />} title={doc.kind === 'cancellation' ? 'Stornorechnung' : 'Storniert'}>
                {doc.kind === 'cancellation' ? 'Dieses Dokument storniert eine Rechnung.' : 'Diese Rechnung wurde storniert.'}
              </Alert>
            )}
            {isInvoiceLike && doc.kind !== 'cancellation' && doc.status !== 'cancelled' && (
              <Card>
                <Group justify="space-between" align="flex-start">
                  <Box>
                    <Text size="sm" c="dimmed">
                      {remaining > 0 ? (refund ? 'Offene Erstattung' : 'Offener Betrag') : 'Vollständig bezahlt'}
                    </Text>
                    <Text fz={28} fw={700} className="tabular" c={remaining > 0 ? (data.days_overdue > 0 ? 'red.7' : undefined) : 'teal.7'}>
                      {money(remaining > 0 ? remaining : data.open_amount)}
                    </Text>
                    {doc.due_date && remaining > 0 && (
                      <Text size="sm" c={data.days_overdue > 0 ? 'red.7' : 'dimmed'}>
                        fällig {date(doc.due_date)} ({relativeDays(doc.due_date)})
                      </Text>
                    )}
                    {doc.paid_at && remaining <= 0 && (
                      <Text size="sm" c="dimmed">
                        bezahlt am {date(doc.paid_at)}
                      </Text>
                    )}
                  </Box>
                  <ThemeIcon size={44} radius="xl" variant="light" color={remaining > 0 ? (data.days_overdue > 0 ? 'red' : 'blue') : 'teal'}>
                    {remaining > 0 ? <IconCash size={22} /> : <IconCircleCheck size={22} />}
                  </ThemeIcon>
                </Group>
                {doc.gross_total > 0 && (
                  <Box mt="md" h={8} bg="var(--mantine-color-gray-2)" style={{ borderRadius: 4, overflow: 'hidden' }}>
                    <Box h="100%" w={`${Math.min(100, (doc.paid_total / doc.gross_total) * 100)}%`} bg="teal.6" />
                  </Box>
                )}
                {data.days_overdue > 0 && (
                  <Button mt="md" fullWidth variant="light" color="orange" leftSection={<IconBellRinging size={16} />} onClick={() => setDunningOpen(true)}>
                    {data.dunnings.length ? 'Nächste Mahnstufe erstellen' : 'Zahlungserinnerung erstellen'}
                  </Button>
                )}
              </Card>
            )}

            {doc.doc_type === 'quote' && doc.status === 'open' && (
              <Card>
                <Text fw={600} mb={4}>
                  Wie hat der Kunde entschieden?
                </Text>
                <Text size="sm" c="dimmed" mb="sm">
                  {doc.valid_until ? `Das Angebot ist gültig bis ${date(doc.valid_until)} (${relativeDays(doc.valid_until)}).` : 'Halten Sie fest, ob das Angebot angenommen wurde.'}
                </Text>
                <Group grow>
                  <Button variant="light" color="teal" leftSection={<IconCheck size={16} />} onClick={() => status.mutate('accepted')} loading={status.isPending}>
                    Angenommen
                  </Button>
                  <Button variant="default" leftSection={<IconX size={16} />} onClick={() => status.mutate('rejected')} loading={status.isPending}>
                    Abgelehnt
                  </Button>
                </Group>
              </Card>
            )}

            <SectionCard title="Details">
              <InfoRow label="Nummer">{doc.number}</InfoRow>
              <InfoRow label="Datum">{date(doc.date)}</InfoRow>
              {doc.service_date && <InfoRow label={doc.service_date_end ? 'Leistungszeitraum' : 'Leistungsdatum'}>{doc.service_date_end ? `${date(doc.service_date)} – ${date(doc.service_date_end)}` : date(doc.service_date)}</InfoRow>}
              {doc.valid_until && <InfoRow label="Gültig bis">{date(doc.valid_until)}</InfoRow>}
              {doc.due_date && <InfoRow label={doc.payment_method === 'debit' ? 'Einzug am' : 'Fällig am'}>{date(doc.due_date)}</InfoRow>}
              {doc.payment_method === 'debit' && (
                <InfoRow label="Zahlungsart">
                  <Tooltip label={mandate ? `${formatIban(mandate.mandate.iban)} · ${mandate.status_label}` : ''} disabled={!mandate}>
                    <span>SEPA-Lastschrift{mandate ? ` · ${mandate.mandate.reference}` : ''}</span>
                  </Tooltip>
                </InfoRow>
              )}
              {doc.reference && <InfoRow label="Referenz">{doc.reference}</InfoRow>}
              {doc.buyer_reference && <InfoRow label="Leitweg-ID">{doc.buyer_reference}</InfoRow>}
              <Divider my="xs" />
              {doc.tax_mode === 'standard' && <InfoRow label="Netto">{money(doc.net_total)}</InfoRow>}
              {doc.tax_mode === 'standard' && <InfoRow label="Umsatzsteuer">{money(doc.tax_total)}</InfoRow>}
              <InfoRow label="Gesamt">
                <Text fw={700} span className="tabular">
                  {money(doc.gross_total)}
                </Text>
              </InfoRow>
              {doc.sent_at && <InfoRow label="Versendet">{dateTime(doc.sent_at)}</InfoRow>}
              {isInvoiceLike && (
                <InfoRow label="Format">
                  <Badge variant="light" color={doc.einvoice_format === 'pdf' ? 'gray' : 'violet'}>
                    {doc.einvoice_format === 'zugferd' ? 'ZUGFeRD' : doc.einvoice_format === 'xrechnung' ? 'XRechnung' : 'PDF'}
                  </Badge>
                </InfoRow>
              )}
              {doc.notes && (
                <Alert mt="sm" variant="light" color="yellow" py="xs">
                  <Text size="sm">{doc.notes}</Text>
                </Alert>
              )}
            </SectionCard>

            {isInvoiceLike && (
              <SectionCard
                title="Zahlungen"
                actions={
                  canPay && (
                    <Button size="compact-sm" variant="light" onClick={() => setPayOpen(true)}>
                      Erfassen
                    </Button>
                  )
                }
              >
                {data.payments.length === 0 ? (
                  <Text size="sm" c="dimmed">
                    Noch keine Zahlungen erfasst.
                  </Text>
                ) : (
                  <Stack gap="xs">
                    {data.payments.map((p) => (
                      <Group key={p.allocation.id} justify="space-between" wrap="nowrap">
                        <Box>
                          <Text size="sm" fw={500}>
                            {p.allocation.kind === 'writeoff' ? p.allocation.description : p.account_name}
                          </Text>
                          <Text size="xs" c="dimmed">
                            {date(p.allocation.date)}
                            {p.transaction?.purpose ? ` · ${p.transaction.purpose}` : ''}
                          </Text>
                        </Box>
                        <Group gap={4} wrap="nowrap">
                          <Money value={Math.abs(p.allocation.amount)} fw={600} c={p.allocation.kind === 'writeoff' ? 'orange.8' : 'teal.7'} />
                          <Tooltip label="Zahlung entfernen">
                            <ActionIcon
                              size="sm"
                              color="red"
                              onClick={() => confirmAction({ title: 'Zahlung entfernen', message: 'Die Zuordnung dieser Zahlung wird entfernt.', danger: true, confirmLabel: 'Entfernen', onConfirm: () => removePayment.mutate(p.allocation.id) })}
                              aria-label="Zahlung entfernen"
                            >
                              <IconTrash size={14} />
                            </ActionIcon>
                          </Tooltip>
                        </Group>
                      </Group>
                    ))}
                  </Stack>
                )}
              </SectionCard>
            )}

            {data.dunnings.length > 0 && (
              <SectionCard title="Mahnungen">
                <Stack gap="xs">
                  {data.dunnings.map((d) => (
                    <Group key={d.id} justify="space-between" wrap="nowrap">
                      <Box>
                        <Text size="sm" fw={500}>
                          {d.title}
                        </Text>
                        <Text size="xs" c="dimmed">
                          {date(d.date)} · Frist {date(d.due_date)} · Gebühr {money(d.fee)}
                        </Text>
                      </Box>
                      <Group gap={2}>
                        <ActionIcon onClick={() => openFile(`/dunning/${d.id}/pdf`)} aria-label="PDF öffnen">
                          <IconExternalLink size={16} />
                        </ActionIcon>
                        <ActionIcon
                          color="red"
                          onClick={() => confirmAction({ title: 'Mahnung löschen', message: `„${d.title}“ vom ${date(d.date)} wird gelöscht.`, danger: true, confirmLabel: 'Löschen', onConfirm: () => removeDunning.mutate(d.id) })}
                          aria-label="Mahnung löschen"
                        >
                          <IconTrash size={14} />
                        </ActionIcon>
                      </Group>
                    </Group>
                  ))}
                </Stack>
              </SectionCard>
            )}

            {recurring && (
              <SectionCard
                title={<Group gap={6}><IconRepeat size={16} />Abo-Rechnung</Group>}
                actions={
                  <Button size="compact-sm" variant="light" onClick={() => setRecurringOpen(true)}>
                    Bearbeiten
                  </Button>
                }
              >
                <Text size="sm">
                  {CYCLES.find((c) => c.value === recurring.cycle)?.label ?? recurring.cycle}
                  {recurring.active ? ` · nächste Rechnung am ${date(recurring.next_date)}` : ' · beendet'}
                  {recurring.end_date ? ` · bis ${date(recurring.end_date)}` : ''}
                </Text>
                <Text size="xs" c="dimmed">
                  {recurringMode(recurring)}
                  {recurring.price_change_percent !== 0 && recurring.price_change_from
                    ? ` · Preise ${recurring.price_change_percent > 0 ? '+' : ''}${recurring.price_change_percent.toLocaleString('de-DE')} % ab ${date(recurring.price_change_from)}${recurring.price_change_yearly ? ', jährlich' : ''}`
                    : ''}
                </Text>
                <RecurringError message={recurring.last_error} />
              </SectionCard>
            )}

            {data.related.length > 0 && (
              <SectionCard title="Verknüpfte Dokumente">
                <Stack gap={6}>
                  {data.related.map((r) => (
                    <Group key={r.id} justify="space-between" wrap="nowrap">
                      <Anchor component={Link} to={`/dokumente/${r.id}`} size="sm">
                        <Group gap={6} wrap="nowrap">
                          <IconArrowRight size={14} />
                          {docLabel(r.doc_type, r.kind)} {r.number || '(Entwurf)'}
                        </Group>
                      </Anchor>
                      <Group gap={6} wrap="nowrap">
                        <Text size="xs" c="dimmed" className="tabular">
                          {money(r.gross_total)}
                        </Text>
                        <StatusBadge status={r.status} docType={r.doc_type} size="sm" />
                      </Group>
                    </Group>
                  ))}
                </Stack>
              </SectionCard>
            )}

            <SectionCard title={<Group gap={6}><IconHistory size={16} />Verlauf</Group>}>
              <Timeline bulletSize={14} lineWidth={2}>
                {data.activities.map((a) => (
                  <Timeline.Item key={a.id} color={ACTIVITY_COLORS[a.action] ?? 'gray'} title={<Text size="sm">{a.message}</Text>}>
                    <Text size="xs" c="dimmed">
                      {dateTime(a.created_at)}
                    </Text>
                  </Timeline.Item>
                ))}
              </Timeline>
            </SectionCard>
          </Stack>
        </Grid.Col>
      </Grid>

      {canPay && (
        <PaymentModal
          opened={payOpen}
          onClose={() => setPayOpen(false)}
          target="documents"
          id={doc.id}
          openAmount={remaining}
          fees={data.dunning_fees}
          incoming={!refund}
          title={`${refund ? 'Erstattung' : 'Zahlung'} für ${label} ${doc.number}`}
        />
      )}
      {canPartial && <PartialModal opened={partialOpen} onClose={() => setPartialOpen(false)} doc={data} />}
      {isInvoice && <DunningModal opened={dunningOpen} onClose={() => setDunningOpen(false)} doc={data} />}
      <SendEmailModal opened={sendOpen} onClose={() => setSendOpen(false)} doc={data} />
      {isInvoice && <RecurringModal opened={recurringOpen} onClose={() => setRecurringOpen(false)} templateId={doc.id} templateDate={doc.date} existing={recurring} contactEmail={data.contact?.email} />}
    </Page>
  );
}
