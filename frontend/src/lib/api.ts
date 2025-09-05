//! ABOUTME: API client with authentication and error handling
//! ABOUTME: Provides typed HTTP client for Glimpser backend API

import type { components } from '@/types/api';

// Type definitions from generated API types
export type LoginRequest = components['schemas']['LoginRequest'];
export type LoginResponse = components['schemas']['LoginResponse'];
export type UserInfo = components['schemas']['UserInfo'];
export type ErrorResponse = components['schemas']['ErrorResponse'];
export type AdminStreamInfo = {
  id: string;
  name: string;
  description?: string;
  config: Record<string, unknown>;
  is_default: boolean;
};

const API_BASE_URL = process.env.NODE_ENV === 'production'
  ? '' // Use same origin in production
  : 'http://localhost:3000'; // Use Next.js proxy in development

class ApiClient {
  private baseURL: string;
  private accessToken: string | null = null;

  constructor(baseURL: string = API_BASE_URL) {
    this.baseURL = baseURL;

    // Try to load token from localStorage on client side
    if (typeof window !== 'undefined') {
      this.accessToken = localStorage.getItem('access_token');
    }
  }

  setAuthToken(token: string) {
    this.accessToken = token;
    if (typeof window !== 'undefined') {
      localStorage.setItem('access_token', token);
    }
  }

  clearAuthToken() {
    this.accessToken = null;
    if (typeof window !== 'undefined') {
      localStorage.removeItem('access_token');
    }
  }

  private async request<T>(
    endpoint: string,
    options: RequestInit = {}
  ): Promise<T> {
    const url = `${this.baseURL}/api${endpoint}`;

    const headers: HeadersInit = {
      ...(options.headers || {}),
    } as HeadersInit;

    // Only set Content-Type for requests with bodies
    const headersObj = headers as Record<string, string>;
    if (options.body && !headersObj['Content-Type'] && !headersObj['content-type']) {
      headersObj['Content-Type'] = 'application/json';
    }

    if (this.accessToken) {
      headersObj['Authorization'] = `Bearer ${this.accessToken}`;
    }

    const response = await fetch(url, {
      ...options,
      headers,
    });

    if (!response.ok) {
      const errorText = await response.text();
      let errorData;

      try {
        errorData = JSON.parse(errorText);
      } catch {
        errorData = { message: errorText };
      }

      throw new ApiError(response.status, errorData);
    }

    const contentType = response.headers.get('content-type');
    if (contentType?.includes('application/json')) {
      return response.json();
    }

    return response.text() as T;
  }

  // Auth endpoints
  async login(email: string, password: string): Promise<LoginResponse> {
    return this.request<LoginResponse>('/auth/login', {
      method: 'POST',
      body: JSON.stringify({ email, password } as LoginRequest),
    });
  }

  // User endpoints
  async me(): Promise<UserInfo> {
    return this.request<UserInfo>('/me');
  }

  async health() {
    return this.request('/health');
  }

  // Stream endpoints
  async streams() {
    return this.request('/streams');
  }

  async alerts() {
    return this.request('/alerts');
  }

  // GET methods
  get<T>(endpoint: string) {
    return this.request<T>(endpoint, { method: 'GET' });
  }

  // POST methods
  post<T>(endpoint: string, data?: unknown) {
    return this.request<T>(endpoint, {
      method: 'POST',
      body: data ? JSON.stringify(data) : undefined,
    });
  }

  // PUT methods
  put<T>(endpoint: string, data?: unknown) {
    return this.request<T>(endpoint, {
      method: 'PUT',
      body: data ? JSON.stringify(data) : undefined,
    });
  }

  // DELETE methods
  delete<T>(endpoint: string) {
    return this.request<T>(endpoint, { method: 'DELETE' });
  }

  // Stream endpoints (listing)
  async getStreams(params?: { page?: number; page_size?: number; search?: string }) {
    const query = params ? new URLSearchParams(
      Object.entries(params).filter(([, v]) => v !== undefined).map(([k, v]) => [k, String(v)])
    ).toString() : '';
    return this.request(`/streams${query ? `?${query}` : ''}`);
  }

  async getStream(id: string) {
    return this.request(`/streams/${id}`);
  }

  async createStream(stream: {
    name: string;
    description?: string;
    config: Record<string, unknown>;
    is_default?: boolean;
  }) {
    return this.post('/streams', stream);
  }

  async updateStream(id: string, stream: {
    name?: string;
    description?: string;
    config?: Record<string, unknown>;
    is_default?: boolean;
  }) {
    return this.put(`/streams/${id}`, stream);
  }

  async deleteStream(id: string) {
    return this.delete(`/streams/${id}`);
  }

  // Stream control endpoints
  async startStream(id: string) {
    return this.post(`/stream/${id}/start`);
  }

  async stopStream(id: string) {
    return this.post(`/stream/${id}/stop`);
  }
}

export class ApiError extends Error {
  constructor(
    public status: number,
    public data: Record<string, unknown>
  ) {
    const message = typeof data.message === 'string' ? data.message : `HTTP ${status}`;
    super(message);
    this.name = 'ApiError';
  }
}

export const apiClient = new ApiClient();
