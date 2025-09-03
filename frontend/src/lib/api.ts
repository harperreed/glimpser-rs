//! ABOUTME: API client with authentication and error handling
//! ABOUTME: Provides typed HTTP client for Glimpser backend API

import type { components, operations } from '@/types/api';

// Type definitions from generated API types
export type LoginRequest = components['schemas']['LoginRequest'];
export type LoginResponse = components['schemas']['LoginResponse'];
export type UserInfo = components['schemas']['UserInfo'];
export type ErrorResponse = components['schemas']['ErrorResponse'];
export type TemplateInfo = components['schemas']['TemplateInfo'];

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
      ...options.headers,
    };

    // Only set Content-Type for requests with bodies
    if (options.body && !headers['Content-Type'] && !headers['content-type']) {
      headers['Content-Type'] = 'application/json';
    }

    if (this.accessToken) {
      headers['Authorization'] = `Bearer ${this.accessToken}`;
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
  post<T>(endpoint: string, data?: any) {
    return this.request<T>(endpoint, {
      method: 'POST',
      body: data ? JSON.stringify(data) : undefined,
    });
  }

  // PUT methods
  put<T>(endpoint: string, data?: any) {
    return this.request<T>(endpoint, {
      method: 'PUT',
      body: data ? JSON.stringify(data) : undefined,
    });
  }

  // DELETE methods
  delete<T>(endpoint: string) {
    return this.request<T>(endpoint, { method: 'DELETE' });
  }
}

export class ApiError extends Error {
  constructor(
    public status: number,
    public data: any
  ) {
    super(data.message || `HTTP ${status}`);
    this.name = 'ApiError';
  }
}

export const apiClient = new ApiClient();
