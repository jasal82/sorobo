// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { useEffect, useState } from 'react';
import { useNavigate } from 'react-router';
import { useQueryClient } from '@tanstack/react-query';
import {
  Box,
  Button,
  Card,
  Center,
  Checkbox,
  Group,
  List,
  Loader,
  Radio,
  Select,
  SimpleGrid,
  Stack,
  Stepper,
  Switch,
  Text,
  TextInput,
  ThemeIcon,
  Title,
  UnstyledButton,
} from '@mantine/core';
import { IconArrowRight, IconBuildingStore, IconCheck, IconRocket, IconSparkles } from '@tabler/icons-react';
import { api } from '@/api/client';
import { notifyError, useSettings, useStatus } from '@/api/hooks';
import { SoroboLogo } from '@/components/ui';
import type { Settings, SystemStatus } from '@/api/types';
import { MoneyInput } from '@/components/inputs';
import { BOOKKEEPING, COUNTRIES } from '@/lib/labels';

export function OnboardingPage() {
  const navigate = useNavigate();
  const qc = useQueryClient();
  const { data, isFetching } = useSettings();
  const { data: status, isFetching: statusFetching } = useStatus();
  const [s, setS] = useState<Settings | null>(null);
  const [busy, setBusy] = useState(false);
  const [step, setStep] = useState(0);
  const [bank, setBank] = useState({ name: 'Geschäftskonto', iban: '', opening: 0 });
  const [cash, setCash] = useState({ enabled: true, opening: 0 });
  const [demo, setDemo] = useState(false);

  // Initialise from freshly fetched settings only (never from a stale cache).
  useEffect(() => {
    if (data && !s && !isFetching) setS(data);
  }, [data, s, isFetching]);

  useEffect(() => {
    if (status?.onboarded && !statusFetching && !busy) navigate('/', { replace: true });
  }, [status, statusFetching, busy, navigate]);

  if (!s) {
    return (
      <Center h="100vh">
        <Loader />
      </Center>
    );
  }
  const c = s.company;
  const setC = (p: Partial<Settings['company']>) => setS({ ...s, company: { ...c, ...p } });
  const setT = (p: Partial<Settings['tax']>) => setS({ ...s, tax: { ...s.tax, ...p } });

  const finish = async (withDemo: boolean) => {
    setBusy(true);
    try {
      await api.post<SystemStatus>('/system/onboarding', {
        settings: { ...s, company: { ...c, iban: c.iban || bank.iban } },
        bank_name: bank.name,
        bank_iban: bank.iban,
        bank_opening: bank.opening,
        with_cash: cash.enabled,
        cash_opening: cash.opening,
        demo: withDemo,
      });
      // Drop every cached query so that no page sees pre-onboarding data.
      qc.clear();
      navigate('/', { replace: true });
    } catch (e) {
      notifyError(e, 'Einrichtung fehlgeschlagen');
    } finally {
      setBusy(false);
    }
  };

  return (
    <Box mih="100vh" bg="var(--app-bg)" py={40} px="md">
      <Box maw={860} mx="auto">
        <Group gap="sm" mb="xl">
          <SoroboLogo size={40} />
          <div>
            <Title order={2}>Willkommen bei Sorobo</Title>
            <Text c="dimmed" size="sm">
              Rechnungen, Belege und Buchhaltung – lokal auf Ihrem Rechner.
            </Text>
          </div>
        </Group>
        <Card padding="xl">
          <Stepper active={step} onStepClick={setStep} allowNextStepsSelect={false} size="sm" mb="xl">
            <Stepper.Step label="Start" />
            <Stepper.Step label="Unternehmen" />
            <Stepper.Step label="Steuern" />
            <Stepper.Step label="Konten" />
            <Stepper.Step label="Fertig" />
          </Stepper>

          {step === 0 && (
            <Stack gap="lg">
              <Text>Wie möchten Sie beginnen?</Text>
              <SimpleGrid cols={{ base: 1, sm: 2 }}>
                <UnstyledButton onClick={() => setStep(1)}>
                  <Card withBorder padding="lg" h="100%" style={{ borderColor: 'var(--mantine-color-brand-4)' }}>
                    <ThemeIcon size={42} radius="md" variant="light" mb="sm">
                      <IconBuildingStore size={24} />
                    </ThemeIcon>
                    <Text fw={650}>Mit meinem Unternehmen starten</Text>
                    <Text size="sm" c="dimmed" mt={4}>
                      In vier kurzen Schritten richten Sie Firmendaten, Steuern und Konten ein.
                    </Text>
                  </Card>
                </UnstyledButton>
                <UnstyledButton onClick={() => finish(true)} disabled={busy}>
                  <Card withBorder padding="lg" h="100%">
                    <ThemeIcon size={42} radius="md" variant="light" color="grape" mb="sm">
                      {busy ? <Loader size="sm" /> : <IconSparkles size={24} />}
                    </ThemeIcon>
                    <Text fw={650}>Erst einmal ausprobieren</Text>
                    <Text size="sm" c="dimmed" mt={4}>
                      Lädt ein Musterunternehmen mit einem vollständigen Geschäftsjahr. Später können Sie alle Daten löschen.
                    </Text>
                  </Card>
                </UnstyledButton>
              </SimpleGrid>
            </Stack>
          )}

          {step === 1 && (
            <Stack gap="sm">
              <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="sm">
                <TextInput label="Firmenname" required value={c.name} onChange={(e) => setC({ name: e.currentTarget.value })} data-autofocus />
                <Select label="Rechtsform" data={['Einzelunternehmen', 'Freiberufler', 'GbR', 'GmbH', 'UG (haftungsbeschränkt)', 'GmbH & Co. KG', 'e.K.', 'AG']} value={c.legal_form || null} onChange={(v) => setC({ legal_form: v ?? '' })} />
                <TextInput label="Inhaber / Geschäftsführung" value={c.owner} onChange={(e) => setC({ owner: e.currentTarget.value })} />
                <TextInput label="Straße und Hausnummer" value={c.street} onChange={(e) => setC({ street: e.currentTarget.value })} />
                <SimpleGrid cols={2} spacing="sm">
                  <TextInput label="PLZ" value={c.zip} onChange={(e) => setC({ zip: e.currentTarget.value })} />
                  <TextInput label="Ort" value={c.city} onChange={(e) => setC({ city: e.currentTarget.value })} />
                </SimpleGrid>
                <Select label="Land" data={COUNTRIES} value={c.country} onChange={(v) => setC({ country: v ?? 'DE' })} />
                <TextInput label="E-Mail" value={c.email} onChange={(e) => setC({ email: e.currentTarget.value })} />
                <TextInput label="Telefon" value={c.phone} onChange={(e) => setC({ phone: e.currentTarget.value })} />
                <TextInput label="Steuernummer" value={c.tax_number} onChange={(e) => setC({ tax_number: e.currentTarget.value })} />
                <TextInput label="USt-IdNr." value={c.vat_id} onChange={(e) => setC({ vat_id: e.currentTarget.value })} />
              </SimpleGrid>
              <Group justify="space-between" mt="md">
                <Button variant="default" onClick={() => setStep(0)}>
                  Zurück
                </Button>
                <Button rightSection={<IconArrowRight size={16} />} onClick={() => setStep(2)} disabled={!c.name}>
                  Weiter
                </Button>
              </Group>
            </Stack>
          )}

          {step === 2 && (
            <Stack gap="md">
              <Switch size="md" label="Ich bin Kleinunternehmer nach § 19 UStG" description="Umsatz im Vorjahr unter 25.000 € und im laufenden Jahr voraussichtlich unter 100.000 €" checked={s.tax.small_business} onChange={(e) => setT({ small_business: e.currentTarget.checked })} />
              <Radio.Group value={s.tax.bookkeeping} onChange={(v) => setT({ bookkeeping: v })} label="Art der Buchführung">
                <SimpleGrid cols={{ base: 1, sm: 2 }} mt="xs">
                  {BOOKKEEPING.map((b) => (
                    <Radio.Card key={b.value} value={b.value} p="md" radius="md">
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
              <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="sm">
                <Select
                  label="Kontenrahmen"
                  data={[
                    { value: 'SKR03', label: 'SKR03 – üblich für Freiberufler und Gewerbe' },
                    { value: 'SKR04', label: 'SKR04 – Abschlussgliederung (Kapitalgesellschaften)' },
                  ]}
                  value={s.tax.chart}
                  onChange={(v) => setT({ chart: v ?? 'SKR03' })}
                />
                <Select
                  label="Umsatzsteuer-Voranmeldung"
                  disabled={s.tax.small_business}
                  data={[
                    { value: 'monthly', label: 'Monatlich' },
                    { value: 'quarterly', label: 'Vierteljährlich' },
                    { value: 'yearly', label: 'Jährlich' },
                  ]}
                  value={s.tax.ustva_period}
                  onChange={(v) => setT({ ustva_period: v ?? 'quarterly' })}
                />
              </SimpleGrid>
              <Group justify="space-between" mt="md">
                <Button variant="default" onClick={() => setStep(1)}>
                  Zurück
                </Button>
                <Button rightSection={<IconArrowRight size={16} />} onClick={() => setStep(3)}>
                  Weiter
                </Button>
              </Group>
            </Stack>
          )}

          {step === 3 && (
            <Stack gap="md">
              <Text fw={600}>Geschäftskonto</Text>
              <SimpleGrid cols={{ base: 1, sm: 3 }} spacing="sm">
                <TextInput label="Bezeichnung" value={bank.name} onChange={(e) => setBank({ ...bank, name: e.currentTarget.value })} />
                <TextInput
                  label="IBAN"
                  value={bank.iban}
                  onChange={(e) => {
                    setBank({ ...bank, iban: e.currentTarget.value });
                    setC({ iban: e.currentTarget.value });
                  }}
                />
                <MoneyInput label={`Kontostand am 01.01.${new Date().getFullYear()}`} value={bank.opening} onChange={(v) => setBank({ ...bank, opening: v })} />
              </SimpleGrid>
              <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="sm">
                <TextInput label="Bank" value={c.bank_name} onChange={(e) => setC({ bank_name: e.currentTarget.value })} />
                <TextInput label="BIC" value={c.bic} onChange={(e) => setC({ bic: e.currentTarget.value })} />
              </SimpleGrid>
              <Switch mt="md" label="Ich führe eine Kasse (Bargeschäfte)" checked={cash.enabled} onChange={(e) => setCash({ ...cash, enabled: e.currentTarget.checked })} />
              {cash.enabled && <MoneyInput label="Kassenbestand zum Jahresbeginn" value={cash.opening} onChange={(v) => setCash({ ...cash, opening: v })} w={260} />}
              <Group justify="space-between" mt="md">
                <Button variant="default" onClick={() => setStep(2)}>
                  Zurück
                </Button>
                <Button rightSection={<IconArrowRight size={16} />} onClick={() => setStep(4)}>
                  Weiter
                </Button>
              </Group>
            </Stack>
          )}

          {step === 4 && (
            <Stack gap="md" align="center" py="md">
              <ThemeIcon size={64} radius="xl" color="teal" variant="light">
                <IconCheck size={36} />
              </ThemeIcon>
              <Title order={3}>Alles bereit!</Title>
              <List spacing={6} size="sm" icon={<IconCheck size={14} color="var(--mantine-color-teal-6)" />}>
                <List.Item>{c.name || 'Ihr Unternehmen'}</List.Item>
                <List.Item>{BOOKKEEPING.find((b) => b.value === s.tax.bookkeeping)?.label} · {s.tax.chart}</List.Item>
                <List.Item>{s.tax.small_business ? 'Kleinunternehmerregelung' : 'Regelbesteuerung'}</List.Item>
                <List.Item>
                  {bank.name}
                  {cash.enabled ? ' und Kasse' : ''}
                </List.Item>
              </List>
              <Checkbox label="Zusätzlich Beispieldaten laden (zum Kennenlernen)" checked={demo} onChange={(e) => setDemo(e.currentTarget.checked)} />
              <Group mt="md">
                <Button variant="default" onClick={() => setStep(3)}>
                  Zurück
                </Button>
                <Button size="md" leftSection={<IconRocket size={18} />} onClick={() => finish(demo)} loading={busy}>
                  Los geht's
                </Button>
              </Group>
            </Stack>
          )}
        </Card>
      </Box>
    </Box>
  );
}
