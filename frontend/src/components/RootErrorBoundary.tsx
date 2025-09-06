//! ABOUTME: Root-level error boundary for catastrophic application errors
//! ABOUTME: Provides fallback UI when the entire app crashes and handles error reporting

'use client';

import React, { Component, ReactNode } from 'react';
import { errorReporting } from '@/lib/error-reporting';

interface Props {
  children: ReactNode;
}

interface State {
  hasError: boolean;
  error: Error | null;
  errorId: string | null;
}

export class RootErrorBoundary extends Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = {
      hasError: false,
      error: null,
      errorId: null,
    };
  }

  static getDerivedStateFromError(error: Error): State {
    // Update state so the next render will show the fallback UI
    return {
      hasError: true,
      error,
      errorId: Date.now().toString(36) + Math.random().toString(36).substr(2),
    };
  }

  componentDidCatch(error: Error, errorInfo: React.ErrorInfo) {
    // Report the error
    errorReporting.reportComponentError(
      error,
      { componentStack: errorInfo.componentStack || '' },
      'RootErrorBoundary'
    );

    // Log to console for immediate debugging
    console.error('Root Error Boundary caught an error:', error, errorInfo);
  }

  private handleReload = () => {
    window.location.reload();
  };

  private handleReportProblem = () => {
    if (this.state.error) {
      // Create a detailed error report for user to send
      const errorDetails = {
        message: this.state.error.message,
        stack: this.state.error.stack,
        userAgent: navigator.userAgent,
        url: window.location.href,
        timestamp: new Date().toISOString(),
        errorId: this.state.errorId,
      };

      // Copy to clipboard or provide other reporting mechanism
      if (navigator.clipboard) {
        navigator.clipboard.writeText(JSON.stringify(errorDetails, null, 2));
        alert('Error details copied to clipboard. Please share with support.');
      } else {
        // Fallback for older browsers
        const textarea = document.createElement('textarea');
        textarea.value = JSON.stringify(errorDetails, null, 2);
        document.body.appendChild(textarea);
        textarea.select();
        document.execCommand('copy');
        document.body.removeChild(textarea);
        alert('Error details copied to clipboard. Please share with support.');
      }
    }
  };

  render() {
    if (this.state.hasError) {
      return (
        <div className="min-h-screen bg-gray-50 flex flex-col items-center justify-center px-4 sm:px-6 lg:px-8">
          <div className="max-w-md w-full bg-white shadow-lg rounded-lg p-6">
            {/* Error Icon */}
            <div className="flex justify-center mb-4">
              <div className="w-16 h-16 bg-red-100 rounded-full flex items-center justify-center">
                <svg className="w-8 h-8 text-red-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
                </svg>
              </div>
            </div>

            {/* Error Message */}
            <div className="text-center mb-6">
              <h1 className="text-xl font-semibold text-gray-900 mb-2">
                Something went wrong
              </h1>
              <p className="text-gray-600 text-sm mb-4">
                The application encountered an unexpected error. This has been automatically reported to our team.
              </p>

              {/* Error ID for support reference */}
              {this.state.errorId && (
                <div className="bg-gray-100 rounded px-3 py-2 text-xs text-gray-500 mb-4">
                  Error ID: {this.state.errorId}
                </div>
              )}

              {/* Error details in development */}
              {process.env.NODE_ENV === 'development' && this.state.error && (
                <details className="text-left bg-red-50 rounded p-3 mb-4">
                  <summary className="cursor-pointer text-sm font-medium text-red-800">
                    Error Details (Development)
                  </summary>
                  <pre className="text-xs text-red-700 mt-2 whitespace-pre-wrap break-words">
                    {this.state.error.message}
                    {this.state.error.stack && (
                      <>
                        {'\n\nStack trace:\n'}
                        {this.state.error.stack}
                      </>
                    )}
                  </pre>
                </details>
              )}
            </div>

            {/* Action Buttons */}
            <div className="space-y-3">
              <button
                onClick={this.handleReload}
                className="w-full bg-blue-600 hover:bg-blue-700 text-white font-medium py-2 px-4 rounded-md transition-colors"
              >
                Reload Application
              </button>

              <button
                onClick={this.handleReportProblem}
                className="w-full bg-gray-100 hover:bg-gray-200 text-gray-700 font-medium py-2 px-4 rounded-md transition-colors"
              >
                Copy Error Details
              </button>

              <button
                onClick={() => window.location.href = '/login'}
                className="w-full text-blue-600 hover:text-blue-500 font-medium py-2 px-4 rounded-md transition-colors text-sm"
              >
                Go to Login Page
              </button>
            </div>

            {/* Help Text */}
            <div className="mt-6 text-center">
              <p className="text-xs text-gray-500">
                If this problem persists, please contact support with the error ID above.
              </p>
            </div>
          </div>
        </div>
      );
    }

    return this.props.children;
  }
}
