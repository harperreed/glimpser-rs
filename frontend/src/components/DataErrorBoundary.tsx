//! ABOUTME: Component-level error boundary for data fetching and async operations
//! ABOUTME: Provides graceful degradation for API failures and loading states

'use client';

import React, { Component, ReactNode } from 'react';
import { errorReporting } from '@/lib/error-reporting';

interface Props {
  children: ReactNode;
  componentName: string;
  fallbackComponent?: ReactNode;
  showRefresh?: boolean;
  onRetry?: () => void;
  retryText?: string;
}

interface State {
  hasError: boolean;
  error: Error | null;
  errorId: string | null;
}

export class DataErrorBoundary extends Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = {
      hasError: false,
      error: null,
      errorId: null,
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
      `DataErrorBoundary:${this.props.componentName}`
    );

    console.error(`Data Error Boundary (${this.props.componentName}) caught an error:`, error, errorInfo);
  }

  private handleRetry = () => {
    this.setState({
      hasError: false,
      error: null,
      errorId: null,
    });

    // Call the custom retry function if provided
    if (this.props.onRetry) {
      this.props.onRetry();
    }
  };

  render() {
    if (this.state.hasError) {
      // Use custom fallback if provided
      if (this.props.fallbackComponent) {
        return this.props.fallbackComponent;
      }

      // Default error UI
      return (
        <div className="bg-red-50 border border-red-200 rounded-md p-4">
          <div className="flex items-start">
            <div className="flex-shrink-0">
              <svg className="h-5 w-5 text-red-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
              </svg>
            </div>
            <div className="ml-3 flex-1">
              <h3 className="text-sm font-medium text-red-800 mb-1">
                Failed to load {this.props.componentName}
              </h3>
              <div className="text-sm text-red-700 mb-3">
                <p>There was a problem loading this content. Please try again.</p>

                {/* Error ID for debugging */}
                {this.state.errorId && (
                  <p className="text-xs text-red-600 mt-1 opacity-75">
                    Error ID: {this.state.errorId}
                  </p>
                )}
              </div>

              {/* Development error details */}
              {process.env.NODE_ENV === 'development' && this.state.error && (
                <details className="mb-3">
                  <summary className="cursor-pointer text-xs font-medium text-red-800">
                    Error Details (Development)
                  </summary>
                  <div className="mt-2 text-xs text-red-700 bg-red-100 p-2 rounded">
                    <div className="font-medium">Message:</div>
                    <div className="mb-2">{this.state.error.message}</div>
                    {this.state.error.stack && (
                      <>
                        <div className="font-medium">Stack:</div>
                        <pre className="whitespace-pre-wrap text-xs">{this.state.error.stack}</pre>
                      </>
                    )}
                  </div>
                </details>
              )}

              {/* Action Buttons */}
              <div className="flex space-x-2">
                {(this.props.showRefresh !== false || this.props.onRetry) && (
                  <button
                    onClick={this.handleRetry}
                    className="bg-red-100 hover:bg-red-200 text-red-800 font-medium py-1 px-3 rounded text-sm transition-colors"
                  >
                    {this.props.retryText || 'Try Again'}
                  </button>
                )}

                <button
                  onClick={() => window.location.reload()}
                  className="bg-red-100 hover:bg-red-200 text-red-800 font-medium py-1 px-3 rounded text-sm transition-colors"
                >
                  Refresh Page
                </button>
              </div>
            </div>
          </div>
        </div>
      );
    }

    return this.props.children;
  }
}

// High-Order Component version for easier usage
export function withDataErrorBoundary<P extends object>(
  WrappedComponent: React.ComponentType<P>,
  componentName: string,
  options?: {
    fallbackComponent?: ReactNode;
    showRefresh?: boolean;
    onRetry?: () => void;
    retryText?: string;
  }
) {
  const WithDataErrorBoundary = (props: P) => {
    return (
      <DataErrorBoundary
        componentName={componentName}
        fallbackComponent={options?.fallbackComponent}
        showRefresh={options?.showRefresh}
        onRetry={options?.onRetry}
        retryText={options?.retryText}
      >
        <WrappedComponent {...props} />
      </DataErrorBoundary>
    );
  };

  WithDataErrorBoundary.displayName = `withDataErrorBoundary(${componentName})`;
  return WithDataErrorBoundary;
}
