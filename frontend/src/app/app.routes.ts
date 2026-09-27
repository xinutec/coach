import { Routes } from '@angular/router';

import { Today } from './features/today/today';
import { BalancePage } from './features/balance/balance';
import { LibraryPage } from './features/library/library';
import { LocationsPage } from './features/locations/locations';
import { HistoryPage } from './features/history/history';
import { SettingsPage } from './features/settings/settings';

export const routes: Routes = [
  { path: 'today', title: 'Coach · Today', component: Today, data: { top: true } },
  { path: 'balance', title: 'Coach · Balance', component: BalancePage, data: { top: true } },
  { path: 'library', title: 'Coach · Library', component: LibraryPage, data: { top: true } },
  {
    path: 'locations',
    title: 'Coach · Locations',
    component: LocationsPage,
    // Opened from Settings and from Today's location menu: up returns to whichever.
    data: { up: { path: '/settings', opener: true } },
  },
  { path: 'history', title: 'Coach · History', component: HistoryPage, data: { top: true } },
  { path: 'settings', title: 'Coach · Settings', component: SettingsPage, data: { top: true } },
  { path: '', pathMatch: 'full', redirectTo: 'today' },
  { path: '**', redirectTo: 'today' },
];
