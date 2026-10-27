// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

// Thin typed HTTP client for the Sorobo backend.

export class ApiError extends Error {
  status: number;
  constructor(message: string, status: number) {
    super(message);
    this.status = status;
  }
}

type Params = Record<string, string | number | boolean | null | undefined>;

export function qs(params: Params = {}): string {
  const entries = Object.entries(params).filter(([, v]) => v !== undefined && v !== null && v !== '');
  if (entries.length === 0) return '';
  return '?' + entries.map(([k, v]) => `${encodeURIComponent(k)}=${encodeURIComponent(String(v))}`).join('&');
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const isForm = body instanceof FormData;
  const res = await fetch('/api' + path, {
    method,
    headers: body !== undefined && !isForm ? { 'Content-Type': 'application/json' } : undefined,
    body: body === undefined ? undefined : isForm ? body : JSON.stringify(body),
  });
  if (!res.ok) {
    let message = res.statusText;
    try {
      const data = await res.json();
      if (data && typeof data.error === 'string') message = data.error;
    } catch {
      /* ignore */
    }
    throw new ApiError(message || `Fehler ${res.status}`, res.status);
  }
  const text = await res.text();
  return (text ? JSON.parse(text) : undefined) as T;
}

export const api = {
  get: <T>(path: string, params?: Params) => request<T>('GET', path + qs(params)),
  post: <T>(path: string, body?: unknown) => request<T>('POST', path, body ?? {}),
  put: <T>(path: string, body?: unknown) => request<T>('PUT', path, body ?? {}),
  del: <T>(path: string) => request<T>('DELETE', path),
  upload: <T>(path: string, file: File | Blob, extra: Record<string, string> = {}, filename?: string) => {
    const form = new FormData();
    form.append('file', file, filename ?? (file instanceof File ? file.name : 'upload'));
    for (const [k, v] of Object.entries(extra)) form.append(k, v);
    return request<T>('POST', path, form);
  },
};

/** URL helper for binary endpoints (PDF, XML, ZIP). */
export function apiUrl(path: string, params?: Params): string {
  return '/api' + path + qs(params);
}

/** Opens a PDF/file in a new browser tab. */
export function openFile(path: string, params?: Params) {
  window.open(apiUrl(path, params), '_blank', 'noopener');
}

/** Triggers a download; surfaces backend errors (JSON) instead of saving them. */
export async function downloadFile(path: string, params?: Params): Promise<void> {
  const res = await fetch(apiUrl(path, { ...params, download: true }));
  if (!res.ok) {
    let message = res.statusText;
    try {
      message = (await res.json()).error ?? message;
    } catch {
      /* ignore */
    }
    throw new ApiError(message, res.status);
  }
  const blob = await res.blob();
  const disposition = res.headers.get('Content-Disposition') ?? '';
  const match = /filename\*=UTF-8''([^;]+)/.exec(disposition) ?? /filename="([^"]+)"/.exec(disposition);
  const name = match ? decodeURIComponent(match[1]) : 'download';
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = name;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 2000);
}
