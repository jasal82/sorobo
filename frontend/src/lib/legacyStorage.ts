// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

/** localStorage keys from the time the program was called Sqonto (until version 0.10). */
const RENAMED_KEYS: [from: string, to: string][] = [
  ['sqonto-color-scheme', 'sorobo-color-scheme'],
  ['sqonto.datev.advisor', 'sorobo.datev.advisor'],
  ['sqonto.datev.client', 'sorobo.datev.client'],
];

/** Takes over what the browser remembered under the former name; runs before anything reads it. */
export function takeOverLegacyStorage() {
  try {
    for (const [from, to] of RENAMED_KEYS) {
      const value = localStorage.getItem(from);
      if (value === null) continue;
      if (localStorage.getItem(to) === null) localStorage.setItem(to, value);
      localStorage.removeItem(from);
    }
  } catch {
    // storage unavailable – nothing to take over
  }
}
