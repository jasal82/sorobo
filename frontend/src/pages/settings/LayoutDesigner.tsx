// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useNavigate, useParams } from 'react-router';
import {
  Accordion,
  ActionIcon,
  Badge,
  Box,
  Button,
  Center,
  ColorInput,
  FileButton,
  Grid,
  Group,
  Loader,
  NumberInput,
  Paper,
  ScrollArea,
  SegmentedControl,
  Select,
  SimpleGrid,
  Slider,
  Stack,
  Switch,
  Text,
  TextInput,
  Textarea,
  Tooltip,
} from '@mantine/core';
import { useDebouncedValue, useElementSize } from '@mantine/hooks';
import { IconArrowLeft, IconDeviceFloppy, IconPhoto, IconRefresh, IconStar, IconTrash, IconUpload } from '@tabler/icons-react';
import { useQuery, keepPreviousData } from '@tanstack/react-query';
import { api, apiUrl } from '@/api/client';
import { UnsavedChangesGuard } from '@/components/ui';
import { notifyError, useAction, useApi, useFonts, useSettings } from '@/api/hooks';
import type { FileMeta, Layout, LayoutConfig, Pages } from '@/api/types';

const PAGE_W = 210;
const PAGE_H = 297;

type BoxKey = 'logo' | 'address' | 'info' | 'company';

interface DragState {
  key: BoxKey | 'content';
  mode: 'move' | 'resize';
  startX: number;
  startY: number;
  orig: LayoutConfig;
}

const SWATCHES = ['#1d4ed8', '#2552eb', '#0f766e', '#047857', '#b45309', '#be123c', '#7c3aed', '#0e7490', '#111827', '#475569'];

function UploadField({ label, fileId, onChange, accept, hint }: { label: string; fileId: number | null; onChange: (id: number | null) => void; accept: string; hint?: string }) {
  const [busy, setBusy] = useState(false);
  const upload = async (f: File | null) => {
    if (!f) return;
    setBusy(true);
    try {
      const meta = await api.upload<FileMeta>('/files', f);
      onChange(meta.id);
    } catch (e) {
      notifyError(e);
    } finally {
      setBusy(false);
    }
  };
  return (
    <Box>
      <Text size="sm" fw={500} mb={4}>
        {label}
      </Text>
      <Group gap="xs">
        <FileButton onChange={upload} accept={accept}>
          {(props) => (
            <Button {...props} size="xs" variant="light" leftSection={<IconUpload size={14} />} loading={busy}>
              {fileId ? 'Ersetzen' : 'Hochladen'}
            </Button>
          )}
        </FileButton>
        {fileId && (
          <>
            <Badge variant="light" color="teal">
              hinterlegt
            </Badge>
            <ActionIcon size="sm" color="red" onClick={() => onChange(null)} aria-label="Entfernen">
              <IconTrash size={14} />
            </ActionIcon>
          </>
        )}
      </Group>
      {hint && (
        <Text size="xs" c="dimmed" mt={4}>
          {hint}
        </Text>
      )}
    </Box>
  );
}

export function LayoutDesignerPage() {
  const { id } = useParams();
  const navigate = useNavigate();
  const { data: layout } = useApi<Layout>(`/layouts/${id}`);
  const { data: fonts } = useFonts();
  const { data: settings } = useSettings();
  const [cfg, setCfg] = useState<LayoutConfig | null>(null);
  const [name, setName] = useState('');
  const [docType, setDocType] = useState('invoice');
  const [active, setActive] = useState<BoxKey | 'content' | null>(null);
  const [logoAspect, setLogoAspect] = useState(0.35);
  const [dirty, setDirty] = useState(false);
  const drag = useRef<DragState | null>(null);
  const { ref: canvasWrap, width: wrapWidth } = useElementSize();

  useEffect(() => {
    if (layout) {
      setCfg(layout.config);
      setName(layout.name);
      setDirty(false);
    }
  }, [layout]);

  const logoId = cfg?.logo_file_id ?? settings?.company.logo_file_id ?? null;
  useEffect(() => {
    if (!logoId) return;
    const img = new window.Image();
    img.onload = () => img.naturalWidth && setLogoAspect(img.naturalHeight / img.naturalWidth);
    img.src = apiUrl(`/files/${logoId}`);
  }, [logoId]);

  const [debounced] = useDebouncedValue(cfg, 450);
  const preview = useQuery({
    queryKey: ['layout-preview', debounced ? JSON.stringify(debounced) : '', docType],
    queryFn: () => api.post<Pages>('/layouts/preview', { config: debounced, document_id: null, doc_type: docType }),
    enabled: !!debounced,
    placeholderData: keepPreviousData,
  });

  const save = useAction(() => api.put<Layout>(`/layouts/${id}`, { ...layout, name, config: cfg }), { success: 'Layout gespeichert', onSuccess: () => setDirty(false) });
  const setDefault = useAction(() => api.post(`/layouts/${id}/default`), { success: 'Als Standard-Layout festgelegt' });

  const set = useCallback((p: Partial<LayoutConfig>) => {
    setCfg((c) => (c ? { ...c, ...p } : c));
    setDirty(true);
  }, []);

  // wrapWidth includes the wrapper padding (2 × 16 px) and border; never wider than the screen allows.
  const canvasWidth = Math.min(Math.max(wrapWidth - 34, 240), 760);
  const scale = canvasWidth / PAGE_W; // px per mm
  const canvasHeight = PAGE_H * scale;

  const onPointerDown = (key: BoxKey | 'content', mode: 'move' | 'resize') => (e: React.PointerEvent) => {
    if (!cfg) return;
    e.preventDefault();
    e.stopPropagation();
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
    drag.current = { key, mode, startX: e.clientX, startY: e.clientY, orig: cfg };
    setActive(key);
  };
  const onPointerMove = (e: React.PointerEvent) => {
    const d = drag.current;
    if (!d) return;
    const dx = (e.clientX - d.startX) / scale;
    const dy = (e.clientY - d.startY) / scale;
    const snap = (v: number) => Math.round(v * 2) / 2;
    const clamp = (v: number, lo: number, hi: number) => Math.min(Math.max(v, lo), hi);
    const o = d.orig;
    if (d.key === 'content') {
      set({ content_top: clamp(snap(o.content_top + dy), 40, 200) });
      return;
    }
    const map = {
      logo: ['logo_x', 'logo_y', 'logo_width'],
      address: ['address_x', 'address_y', 'address_width'],
      info: ['info_x', 'info_y', 'info_width'],
      company: ['company_x', 'company_y', 'company_width'],
    } as const;
    const [kx, ky, kw] = map[d.key];
    if (d.mode === 'move') {
      set({ [kx]: clamp(snap(o[kx] + dx), 0, PAGE_W - 10), [ky]: clamp(snap(o[ky] + dy), 0, PAGE_H - 10) } as Partial<LayoutConfig>);
    } else {
      const patch: Partial<LayoutConfig> = { [kw]: clamp(snap(o[kw] + dx), 10, PAGE_W - o[kx]) } as Partial<LayoutConfig>;
      if (d.key === 'address') patch.address_height = clamp(snap(o.address_height + dy), 20, 80);
      set(patch);
    }
  };
  const onPointerUp = () => {
    drag.current = null;
  };

  const boxes = useMemo(() => {
    if (!cfg) return [];
    const list: { key: BoxKey; label: string; x: number; y: number; w: number; h: number }[] = [];
    if (logoId) list.push({ key: 'logo', label: 'Logo', x: cfg.logo_x, y: cfg.logo_y, w: cfg.logo_width, h: cfg.logo_width * logoAspect });
    list.push({ key: 'address', label: 'Anschriftfeld', x: cfg.address_x, y: cfg.address_y, w: cfg.address_width, h: cfg.address_height });
    list.push({ key: 'info', label: 'Infoblock', x: cfg.info_x, y: cfg.info_y, w: cfg.info_width, h: 34 });
    if (cfg.show_company_block) list.push({ key: 'company', label: 'Absenderblock', x: cfg.company_x, y: cfg.company_y, w: cfg.company_width, h: 24 });
    return list;
  }, [cfg, logoId, logoAspect]);

  if (!cfg || !layout) {
    return (
      <Center h="70vh">
        <Loader />
      </Center>
    );
  }

  const fontOptions = [
    { group: 'Empfohlen (eingebettet)', items: fonts?.recommended ?? ['Inter'] },
    { group: 'Systemschriften', items: (fonts?.all ?? []).filter((f) => !(fonts?.recommended ?? []).includes(f)) },
  ];

  return (
    <Box px={{ base: 'md', sm: 'xl' }} py="lg">
      <Group justify="space-between" mb="lg" wrap="wrap">
        <Group gap="sm">
          <ActionIcon variant="default" size="lg" onClick={() => navigate('/einstellungen/layouts')} aria-label="Zurück">
            <IconArrowLeft size={18} />
          </ActionIcon>
          <div>
            <Text size="xs" c="dimmed">
              Layout-Designer
            </Text>
            <TextInput
              value={name}
              onChange={(e) => {
                setName(e.currentTarget.value);
                setDirty(true);
              }}
              variant="unstyled"
              size="lg"
              fw={700}
              styles={{ input: { fontWeight: 700, fontSize: 20, padding: 0, height: 30 } }}
            />
          </div>
          {layout.is_default && (
            <Badge leftSection={<IconStar size={10} />} variant="light">
              Standard
            </Badge>
          )}
        </Group>
        <Group gap="xs">
          <SegmentedControl
            value={docType}
            onChange={setDocType}
            data={[
              { value: 'invoice', label: 'Rechnung' },
              { value: 'quote', label: 'Angebot' },
              { value: 'delivery', label: 'Lieferschein' },
            ]}
          />
          {!layout.is_default && (
            <Button variant="default" leftSection={<IconStar size={16} />} onClick={() => setDefault.mutate()}>
              Als Standard
            </Button>
          )}
          <Button leftSection={<IconDeviceFloppy size={16} />} onClick={() => save.mutate()} loading={save.isPending} disabled={!dirty}>
            Speichern
          </Button>
        </Group>
      </Group>

      <Grid gap="lg">
        <Grid.Col span={{ base: 12, md: 5, xl: 4 }}>
          <Paper withBorder radius="lg" style={{ overflow: 'hidden' }}>
            <ScrollArea h="calc(100vh - 190px)" type="auto">
              <Accordion multiple defaultValue={['general', 'logo', 'elements']} variant="default" chevronPosition="right">
                <Accordion.Item value="general">
                  <Accordion.Control>Schrift & Farben</Accordion.Control>
                  <Accordion.Panel>
                    <Stack gap="sm">
                      <Select label="Schriftart" searchable data={fontOptions} value={cfg.font} onChange={(v) => v && set({ font: v })} />
                      <Box>
                        <Text size="sm" fw={500}>
                          Schriftgröße: {cfg.font_size.toLocaleString('de-DE')} pt
                        </Text>
                        <Slider min={8} max={12} step={0.25} value={cfg.font_size} onChange={(v) => set({ font_size: v })} label={null} />
                      </Box>
                      <Box>
                        <Text size="sm" fw={500}>
                          Titelgröße: {cfg.title_size} pt
                        </Text>
                        <Slider min={11} max={24} step={0.5} value={cfg.title_size} onChange={(v) => set({ title_size: v })} label={null} />
                      </Box>
                      <ColorInput label="Akzentfarbe" value={cfg.accent_color} onChange={(v) => set({ accent_color: v })} swatches={SWATCHES} swatchesPerRow={10} format="hex" />
                      <ColorInput label="Textfarbe" value={cfg.text_color} onChange={(v) => set({ text_color: v })} swatches={['#111827', '#1f2937', '#374151', '#000000']} format="hex" />
                      <Switch label="Farbbalken am oberen Rand" checked={cfg.accent_bar} onChange={(e) => set({ accent_bar: e.currentTarget.checked })} />
                    </Stack>
                  </Accordion.Panel>
                </Accordion.Item>
                <Accordion.Item value="logo">
                  <Accordion.Control>Logo & Briefpapier</Accordion.Control>
                  <Accordion.Panel>
                    <Stack gap="md">
                      <UploadField
                        label="Logo für dieses Layout"
                        fileId={cfg.logo_file_id}
                        onChange={(v) => set({ logo_file_id: v })}
                        accept="image/png,image/jpeg,image/svg+xml,image/webp"
                        hint={!cfg.logo_file_id && settings?.company.logo_file_id ? 'Es wird das Firmenlogo aus den Unternehmensdaten verwendet.' : undefined}
                      />
                      {logoId && (
                        <>
                          <Box>
                            <Text size="sm" fw={500}>
                              Logobreite: {cfg.logo_width} mm
                            </Text>
                            <Slider min={15} max={110} step={1} value={cfg.logo_width} onChange={(v) => set({ logo_width: v })} label={null} />
                          </Box>
                          <Switch label="Logo auch auf Folgeseiten" checked={cfg.logo_all_pages} onChange={(e) => set({ logo_all_pages: e.currentTarget.checked })} />
                        </>
                      )}
                      <UploadField label="Briefpapier erste Seite" fileId={cfg.letterhead_first_id} onChange={(v) => set({ letterhead_first_id: v })} accept="application/pdf,image/png,image/jpeg" hint="PDF oder Bild im A4-Format – wird als Hintergrund verwendet." />
                      <UploadField label="Briefpapier Folgeseiten" fileId={cfg.letterhead_other_id} onChange={(v) => set({ letterhead_other_id: v })} accept="application/pdf,image/png,image/jpeg" />
                    </Stack>
                  </Accordion.Panel>
                </Accordion.Item>
                <Accordion.Item value="elements">
                  <Accordion.Control>Elemente</Accordion.Control>
                  <Accordion.Panel>
                    <Stack gap="sm">
                      <Text size="xs" c="dimmed">
                        Ziehen Sie die markierten Elemente in der Vorschau an die gewünschte Position. Mit dem Griff unten rechts ändern Sie die Größe.
                      </Text>
                      <Switch label="Rücksendezeile über der Anschrift" checked={cfg.show_sender_line} onChange={(e) => set({ show_sender_line: e.currentTarget.checked })} />
                      {cfg.show_sender_line && <TextInput size="xs" placeholder="automatisch aus Firmenanschrift" value={cfg.sender_line} onChange={(e) => set({ sender_line: e.currentTarget.value })} />}
                      <Switch label="Absenderblock (Firmenangaben oben)" checked={cfg.show_company_block} onChange={(e) => set({ show_company_block: e.currentTarget.checked })} />
                      {cfg.show_company_block && (
                        <SegmentedControl
                          size="xs"
                          value={cfg.company_align}
                          onChange={(v) => set({ company_align: v })}
                          data={[
                            { value: 'left', label: 'Linksbündig' },
                            { value: 'right', label: 'Rechtsbündig' },
                          ]}
                        />
                      )}
                      <Switch label="Falz- und Lochmarken" checked={cfg.fold_marks} onChange={(e) => set({ fold_marks: e.currentTarget.checked })} />
                      <Switch label="GiroCode (QR-Code zum Bezahlen)" checked={cfg.show_girocode} onChange={(e) => set({ show_girocode: e.currentTarget.checked })} />
                      <Switch label="Unterschriftsfeld auf Angeboten" checked={cfg.show_signature} onChange={(e) => set({ show_signature: e.currentTarget.checked })} />
                      <Switch label="Seitenzahlen" checked={cfg.show_page_numbers} onChange={(e) => set({ show_page_numbers: e.currentTarget.checked })} />
                      <SimpleGrid cols={3} spacing="xs">
                        <NumberInput size="xs" label="Anschrift X" suffix=" mm" value={cfg.address_x} onChange={(v) => set({ address_x: Number(v) || 0 })} />
                        <NumberInput size="xs" label="Anschrift Y" suffix=" mm" value={cfg.address_y} onChange={(v) => set({ address_y: Number(v) || 0 })} />
                        <NumberInput size="xs" label="Breite" suffix=" mm" value={cfg.address_width} onChange={(v) => set({ address_width: Number(v) || 85 })} />
                        <NumberInput size="xs" label="Info X" suffix=" mm" value={cfg.info_x} onChange={(v) => set({ info_x: Number(v) || 0 })} />
                        <NumberInput size="xs" label="Info Y" suffix=" mm" value={cfg.info_y} onChange={(v) => set({ info_y: Number(v) || 0 })} />
                        <NumberInput size="xs" label="Breite" suffix=" mm" value={cfg.info_width} onChange={(v) => set({ info_width: Number(v) || 60 })} />
                      </SimpleGrid>
                      <Button
                        size="xs"
                        variant="subtle"
                        leftSection={<IconRefresh size={14} />}
                        onClick={() => set({ address_x: 25, address_y: 48, address_width: 85, address_height: 40, info_x: 130, info_y: 52, info_width: 60, content_top: 103 })}
                      >
                        DIN 5008 (Form B) wiederherstellen
                      </Button>
                    </Stack>
                  </Accordion.Panel>
                </Accordion.Item>
                <Accordion.Item value="table">
                  <Accordion.Control>Positionstabelle</Accordion.Control>
                  <Accordion.Panel>
                    <Stack gap="sm">
                      <SegmentedControl
                        fullWidth
                        value={cfg.table_style}
                        onChange={(v) => set({ table_style: v })}
                        data={[
                          { value: 'lines', label: 'Linien' },
                          { value: 'striped', label: 'Streifen' },
                          { value: 'grid', label: 'Gitter' },
                          { value: 'minimal', label: 'Minimal' },
                        ]}
                      />
                      <Switch label="Kopfzeile farbig hinterlegen" checked={cfg.table_header_fill} onChange={(e) => set({ table_header_fill: e.currentTarget.checked })} />
                      <Switch label="Spalte Position" checked={cfg.show_position} onChange={(e) => set({ show_position: e.currentTarget.checked })} />
                      <Switch label="Spalte Artikelnummer" checked={cfg.show_article_number} onChange={(e) => set({ show_article_number: e.currentTarget.checked })} />
                      <Switch label="Spalte Einheit" checked={cfg.show_unit_column} onChange={(e) => set({ show_unit_column: e.currentTarget.checked })} />
                      <Switch label="Spalte USt-Satz" checked={cfg.show_tax_column} onChange={(e) => set({ show_tax_column: e.currentTarget.checked })} />
                    </Stack>
                  </Accordion.Panel>
                </Accordion.Item>
                <Accordion.Item value="footer">
                  <Accordion.Control>Fußzeile</Accordion.Control>
                  <Accordion.Panel>
                    <Stack gap="sm">
                      <Switch label="Fußzeile anzeigen" checked={cfg.show_footer} onChange={(e) => set({ show_footer: e.currentTarget.checked })} />
                      {cfg.show_footer && (
                        <>
                          <SegmentedControl
                            fullWidth
                            size="xs"
                            value={cfg.footer_mode}
                            onChange={(v) => set({ footer_mode: v, footer_custom: v === 'custom' && cfg.footer_custom.length === 0 ? ['', '', '', ''] : cfg.footer_custom })}
                            data={[
                              { value: 'auto', label: 'Automatisch aus Firmendaten' },
                              { value: 'custom', label: 'Eigener Text' },
                            ]}
                          />
                          {cfg.footer_mode === 'custom' &&
                            [0, 1, 2, 3].map((i) => (
                              <Textarea
                                key={i}
                                size="xs"
                                label={`Spalte ${i + 1}`}
                                minRows={2}
                                value={cfg.footer_custom[i] ?? ''}
                                onChange={(e) => {
                                  const cols = [...cfg.footer_custom];
                                  while (cols.length < 4) cols.push('');
                                  cols[i] = e.currentTarget.value;
                                  set({ footer_custom: cols });
                                }}
                              />
                            ))}
                          <Box>
                            <Text size="sm" fw={500}>
                              Schriftgröße: {cfg.footer_size} pt
                            </Text>
                            <Slider min={5.5} max={9} step={0.5} value={cfg.footer_size} onChange={(v) => set({ footer_size: v })} label={null} />
                          </Box>
                        </>
                      )}
                    </Stack>
                  </Accordion.Panel>
                </Accordion.Item>
                <Accordion.Item value="margins">
                  <Accordion.Control>Seitenränder</Accordion.Control>
                  <Accordion.Panel>
                    <SimpleGrid cols={2} spacing="xs">
                      <NumberInput size="xs" label="Oben (Folgeseiten)" suffix=" mm" value={cfg.margin_top} onChange={(v) => set({ margin_top: Number(v) || 20 })} />
                      <NumberInput size="xs" label="Unten" suffix=" mm" value={cfg.margin_bottom} onChange={(v) => set({ margin_bottom: Number(v) || 25 })} />
                      <NumberInput size="xs" label="Links" suffix=" mm" value={cfg.margin_left} onChange={(v) => set({ margin_left: Number(v) || 20 })} />
                      <NumberInput size="xs" label="Rechts" suffix=" mm" value={cfg.margin_right} onChange={(v) => set({ margin_right: Number(v) || 20 })} />
                      <NumberInput size="xs" label="Textbeginn Seite 1" suffix=" mm" value={cfg.content_top} onChange={(v) => set({ content_top: Number(v) || 100 })} />
                    </SimpleGrid>
                  </Accordion.Panel>
                </Accordion.Item>
              </Accordion>
            </ScrollArea>
          </Paper>
        </Grid.Col>
        <Grid.Col span={{ base: 12, md: 7, xl: 8 }}>
          <Box ref={canvasWrap} bg="var(--app-bg)" p="md" style={{ borderRadius: 16, border: '1px solid var(--app-border)' }}>
            <Group justify="space-between" mb="sm" maw={canvasWidth} mx="auto">
              <Text size="xs" c="dimmed">
                A4 · Vorschau mit Beispieldaten {preview.isFetching && <Loader size={10} ml={6} />}
              </Text>
              <Text size="xs" c="dimmed">
                {active ? `${{ logo: 'Logo', address: 'Anschriftfeld', info: 'Infoblock', company: 'Absenderblock', content: 'Textbeginn' }[active]} ausgewählt` : 'Element anklicken und ziehen'}
              </Text>
            </Group>
            <div
              className="designer-canvas"
              style={{ width: canvasWidth, height: canvasHeight }}
              onPointerMove={onPointerMove}
              onPointerUp={onPointerUp}
              onPointerDown={() => setActive(null)}
            >
              <div className="preview-layer" dangerouslySetInnerHTML={{ __html: preview.data?.pages[0] ?? '' }} />
              {!preview.data && (
                <Center pos="absolute" inset={0}>
                  <Loader />
                </Center>
              )}
              {boxes.map((b) => (
                <div
                  key={b.key}
                  className="designer-box"
                  data-active={active === b.key}
                  style={{ left: b.x * scale, top: b.y * scale, width: b.w * scale, height: b.h * scale }}
                  onPointerDown={onPointerDown(b.key, 'move')}
                >
                  <span className="designer-label">
                    {b.label} · {b.x.toLocaleString('de-DE')} / {b.y.toLocaleString('de-DE')} mm
                  </span>
                  <Tooltip label="Größe ändern">
                    <div className="designer-resize" onPointerDown={onPointerDown(b.key, 'resize')} />
                  </Tooltip>
                </div>
              ))}
              <div className="designer-guide" style={{ top: cfg.content_top * scale, height: 10 }} onPointerDown={onPointerDown('content', 'move')}>
                <span>Textbeginn {cfg.content_top.toLocaleString('de-DE')} mm</span>
              </div>
            </div>
            {!logoId && (
              <Group justify="center" mt="sm" gap="xs">
                <IconPhoto size={16} color="var(--app-muted)" />
                <Text size="xs" c="dimmed">
                  Tipp: Laden Sie ein Logo hoch, um es frei zu positionieren.
                </Text>
              </Group>
            )}
          </Box>
        </Grid.Col>
      </Grid>
      <UnsavedChangesGuard when={dirty} onSave={() => save.mutateAsync()} />
    </Box>
  );
}
