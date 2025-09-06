//! ABOUTME: Error reporting and logging service for production debugging
//! ABOUTME: Centralizes error handling with context and user information

interface ErrorContext {
  user?: {
    id: string;
    email: string;
  };
  url?: string;
  userAgent?: string;
  timestamp: string;
  componentStack?: string;
  errorBoundary?: string;
}

interface ErrorReport {
  message: string;
  stack?: string;
  context: ErrorContext;
  level: 'error' | 'warn' | 'info';
}

class ErrorReportingService {
  private isEnabled: boolean;

  constructor() {
    // Enable in production or when explicitly enabled
    this.isEnabled = process.env.NODE_ENV === 'production' ||
                     process.env.NEXT_PUBLIC_ENABLE_ERROR_REPORTING === 'true';
  }

  private buildContext(additionalContext?: Partial<ErrorContext>): ErrorContext {
    const context: ErrorContext = {
      timestamp: new Date().toISOString(),
      ...additionalContext,
    };

    if (typeof window !== 'undefined') {
      context.url = window.location.href;
      context.userAgent = window.navigator.userAgent;
    }

    return context;
  }

  private async sendReport(report: ErrorReport): Promise<void> {
    if (!this.isEnabled) {
      // In development, just log to console
      console.error('Error Report:', report);
      return;
    }

    try {
      // In a real application, you would send this to your error reporting service
      // e.g., Sentry, LogRocket, Bugsnag, or your own endpoint

      // For now, we'll store in localStorage for debugging and log to console
      if (typeof window !== 'undefined') {
        const existingReports = localStorage.getItem('error_reports');
        const reports = existingReports ? JSON.parse(existingReports) : [];

        reports.push(report);

        // Keep only last 50 reports to avoid localStorage bloat
        const recentReports = reports.slice(-50);
        localStorage.setItem('error_reports', JSON.stringify(recentReports));
      }

      console.error('Error Report:', report);

      // TODO: Send to actual error reporting service
      // await fetch('/api/errors', {
      //   method: 'POST',
      //   headers: { 'Content-Type': 'application/json' },
      //   body: JSON.stringify(report),
      // });
    } catch (error) {
      // Fallback: at least log to console if reporting fails
      console.error('Failed to send error report:', error);
      console.error('Original error:', report);
    }
  }

  reportError(
    error: Error,
    additionalContext?: Partial<ErrorContext>,
    level: 'error' | 'warn' | 'info' = 'error'
  ): void {
    const context = this.buildContext(additionalContext);

    const report: ErrorReport = {
      message: error.message,
      stack: error.stack,
      context,
      level,
    };

    this.sendReport(report);
  }

  reportComponentError(
    error: Error,
    errorInfo: { componentStack: string },
    boundaryName: string,
    userContext?: { id: string; email: string }
  ): void {
    this.reportError(error, {
      componentStack: errorInfo.componentStack,
      errorBoundary: boundaryName,
      user: userContext,
    });
  }

  reportApiError(
    error: Error,
    endpoint: string,
    method: string,
    status?: number
  ): void {
    this.reportError(error, {
      url: endpoint,
      userAgent: `${method} ${endpoint} ${status ? `(${status})` : ''}`,
    });
  }

  // Utility to get recent error reports for debugging
  getRecentReports(): ErrorReport[] {
    if (typeof window === 'undefined') return [];

    try {
      const reports = localStorage.getItem('error_reports');
      return reports ? JSON.parse(reports) : [];
    } catch {
      return [];
    }
  }

  // Clear stored error reports
  clearReports(): void {
    if (typeof window !== 'undefined') {
      localStorage.removeItem('error_reports');
    }
  }
}

export const errorReporting = new ErrorReportingService();
export type { ErrorContext, ErrorReport };
