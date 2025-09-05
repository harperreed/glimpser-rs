//! ABOUTME: Authentication context for managing login state
//! ABOUTME: Provides auth state and methods throughout the app

'use client';

import { createContext, useContext, useEffect, useState, ReactNode } from 'react';
import { apiClient, type UserInfo } from '@/lib/api';

type User = UserInfo;

interface AuthContextType {
  user: User | null;
  isLoading: boolean;
  login: (email: string, password: string) => Promise<void>;
  logout: () => void;
  isAuthenticated: boolean;
}

const AuthContext = createContext<AuthContextType | undefined>(undefined);

interface AuthProviderProps {
  children: ReactNode;
}

export function AuthProvider({ children }: AuthProviderProps) {
  const [user, setUser] = useState<User | null>(null);
  const [isLoading, setIsLoading] = useState(true);

  const isAuthenticated = !!user;

  useEffect(() => {
    // Check if user is already logged in
    checkAuth();
  }, []);

  const checkAuth = async () => {
    try {
      const userData = await apiClient.me();
      setUser(userData);
    } catch (error) {
      // User is not authenticated, token is invalid, or network error
      // Clear token and set user to null in all cases
      apiClient.clearAuthToken();
      setUser(null);

      // Log network errors for debugging but don't throw
      if (error instanceof Error && !error.message.includes('401')) {
        console.warn('Auth check failed:', error.message);
      }
    } finally {
      setIsLoading(false);
    }
  };

  const login = async (email: string, password: string) => {
    try {
      const response = await apiClient.login(email, password);
      apiClient.setAuthToken(response.access_token);

      // If response doesn't include user, fetch it separately
      if (response.user) {
        setUser(response.user);
      } else {
        // Fetch user info after login
        const userData = await apiClient.me();
        setUser(userData);
      }
    } catch (error) {
      // Clean up on login failure
      apiClient.clearAuthToken();
      setUser(null);
      throw error;
    }
  };

  const logout = () => {
    apiClient.clearAuthToken();
    setUser(null);
  };

  const value = {
    user,
    isLoading,
    login,
    logout,
    isAuthenticated,
  };

  return (
    <AuthContext.Provider value={value}>
      {children}
    </AuthContext.Provider>
  );
}

export function useAuth() {
  const context = useContext(AuthContext);
  if (context === undefined) {
    throw new Error('useAuth must be used within an AuthProvider');
  }
  return context;
}
