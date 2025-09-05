//! ABOUTME: Unified navigation component for consistent UI across pages
//! ABOUTME: Provides main navigation bar with user info and logout

'use client';

import { useAuth } from '@/contexts/auth';
import { useRouter, usePathname } from 'next/navigation';

export function Navigation() {
  const { user, logout } = useAuth();
  const router = useRouter();
  const pathname = usePathname();

  const handleLogout = () => {
    logout();
    router.push('/login');
  };

  const navItems = [
    { href: '/dashboard', label: 'Dashboard' },
    { href: '/streams', label: 'Live Streams' },
    { href: '/admin', label: 'Admin' },
  ];

  return (
    <nav className="bg-white border-b border-gray-300 px-8 py-4 flex justify-between items-center shadow-sm">
      <div className="flex items-center gap-8">
        <h1 className="text-xl font-bold text-blue-600">🔍 Glimpser</h1>

        <div className="flex items-center gap-4">
          {navItems.map((item) => (
            <button
              key={item.href}
              onClick={() => router.push(item.href)}
              className={`text-sm font-medium transition-colors duration-200 ${
                pathname === item.href
                  ? 'text-blue-600 border-b-2 border-blue-600 pb-1'
                  : 'text-gray-600 hover:text-blue-600'
              }`}
            >
              {item.label}
            </button>
          ))}
        </div>
      </div>

      <div className="flex items-center gap-6">
        <span className="text-sm text-gray-500">
          Welcome, {user?.username || user?.email}
        </span>
        <button
          onClick={handleLogout}
          className="px-4 py-2 bg-red-600 text-white rounded-md text-sm font-medium hover:bg-red-700 transition-all duration-200"
        >
          Logout
        </button>
      </div>
    </nav>
  );
}
