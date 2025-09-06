//! ABOUTME: Route-level error boundary for page-specific error handling
//! ABOUTME: Provides context-aware error recovery without breaking the entire app

'use client';

import React, { Component, ReactNode } from 'react';
import { errorReporting } from '@/lib/error-reporting';

interface Props {
  children: ReactNode;
  routeName: string;
  fallback?: ReactNode;
  showRetry?: boolean;
}

interface State {
  hasError: boolean;
  error: Error | null;
  errorId: string | null;
  retryCount: number;
}

export class RouteErrorBoundary extends Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = {
      hasError: false,
      error: null,
      errorId: null,
      retryCount: 0,
    };
  }

  static getDerivedStateFromError(error: Error): Partial<State> {
    return {
      hasError: true,
      error,
      errorId: Date.now().toString(36) + Math.random().toString(36).substr(2),
    };
  }

  componentDidCatch(error: Error, errorInfo: React.ErrorInfo) {
    errorReporting.reportComponentError(
      error,
      { componentStack: errorInfo.componentStack || '' },
      `RouteErrorBoundary:${this.props.routeName}`
    );

    console.error(`Route Error Boundary (${this.props.routeName}) caught an error:`, error, errorInfo);
  }

  private handleRetry = () => {
    this.setState(prevState => ({
      hasError: false,
      error: null,
      errorId: null,
      retryCount: prevState.retryCount + 1,
    }));
  };

  private handleGoBack = () => {
    if (window.history.length > 1) {
      window.history.back();
    } else {
      window.location.href = '/dashboard';
    }
  };

  private handleGoHome = () => {
    window.location.href = '/dashboard';
  };

  render() {
    if (this.state.hasError) {
      // Use custom fallback if provided
      if (this.props.fallback) {
        return this.props.fallback;
      }

      return (
        <div className="min-h-screen bg-gray-50 flex items-center justify-center px-4 sm:px-6 lg:px-8">
          <div className="max-w-lg w-full">
            <div className="bg-white shadow rounded-lg p-6">
              {/* Error Icon */}
              <div className="flex items-center mb-4">
                <div className="flex-shrink-0">
                  <div className="w-10 h-10 bg-orange-100 rounded-full flex items-center justify-center">
                    <svg className="w-6 h-6 text-orange-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-2.5L13.732 4c-.77-.833-1.964-.833-2.732 0L4.082 16.5c-.77.833.192 2.5 1.732 2.5z" />
                    </svg>
                  </div>
                </div>
                <div className="ml-4">
                  <h3 className="text-lg font-medium text-gray-900">
                    Page Error
                  </h3>
                  <p className="text-sm text-gray-500">
                    The {this.props.routeName} page encountered an error
                  </p>
                </div>
              </div>

              {/* Error Message */}
              <div className="mb-6">
                <p className="text-gray-600 text-sm mb-3">
                  Something went wrong while loading this page. The error has been reported and will be investigated.
                </p>

                {/* Retry Count Warning */}
                {this.state.retryCount > 0 && (
                  <div className="bg-yellow-50 border border-yellow-200 rounded-md p-3 mb-3">
                    <p className="text-yellow-800 text-sm">
                      You&apos;ve retried {this.state.retryCount} time(s). If the problem persists, try navigating to a different page.
                    </p>
                  </div>
                )}

                {/* Error ID */}
                {this.state.errorId && (
                  <div className="bg-gray-100 rounded px-3 py-2 text-xs text-gray-500 mb-3">
                    Error ID: {this.state.errorId}
                  </div>
                )}

                {/* Development Error Details */}
                {process.env.NODE_ENV === 'development' && this.state.error && (
                  <details className="bg-red-50 border border-red-200 rounded p-3 mb-3">
                    <summary className="cursor-pointer text-sm font-medium text-red-800 mb-2">
                      Error Details (Development)
                    </summary>
                    <div className="text-xs text-red-700">
                      <div className="font-medium mb-1">Message:</div>
                      <div className="mb-3">{this.state.error.message}</div>
                      {this.state.error.stack && (
                        <>
                          <div className="font-medium mb-1">Stack Trace:</div>
                          <pre className="whitespace-pre-wrap break-words text-xs">
                            {this.state.error.stack}
                          </pre>
                        </>
                      )}
                    </div>
                  </details>
                )}
              </div>

              {/* Action Buttons */}
              <div className="flex flex-col sm:flex-row gap-3">
                {this.props.showRetry !== false && (
                  <button
                    onClick={this.handleRetry}
                    disabled={this.state.retryCount >= 3}
                    className="flex-1 bg-blue-600 hover:bg-blue-700 disabled:bg-gray-300 disabled:cursor-not-allowed text-white font-medium py-2 px-4 rounded-md transition-colors"
                  >
                    {this.state.retryCount >= 3 ? 'Max Retries Reached' : 'Try Again'}
                  </button>
                )}

                <button
                  onClick={this.handleGoBack}
                  className="flex-1 bg-gray-100 hover:bg-gray-200 text-gray-700 font-medium py-2 px-4 rounded-md transition-colors"
                >
                  Go Back
                </button>

                <button
                  onClick={this.handleGoHome}
                  className="flex-1 bg-gray-100 hover:bg-gray-200 text-gray-700 font-medium py-2 px-4 rounded-md transition-colors"
                >
                  Dashboard
                </button>
              </div>

              {/* Help Text */}
              <div className="mt-4 text-center">
                <p className="text-xs text-gray-500">
                  If this problem continues, please contact support with the error ID above.
                </p>
              </div>
            </div>
          </div>
        </div>
      );
    }

    // Add key based on retry count to force re-render of children
    return (
      <div key={this.state.retryCount}>
        {this.props.children}
      </div>
    );
  }
}
