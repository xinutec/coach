import { Routes } from "@angular/router";

import { Today } from "./features/today/today";
import { ProgramPage } from "./features/program/program";
import { LibraryPage } from "./features/library/library";
import { LocationsPage } from "./features/locations/locations";
import { HistoryPage } from "./features/history/history";
import { SettingsPage } from "./features/settings/settings";

export const routes: Routes = [
  { path: "today", title: "Coach · Today", component: Today },
  { path: "program", title: "Coach · Program", component: ProgramPage },
  { path: "library", title: "Coach · Library", component: LibraryPage },
  { path: "locations", title: "Coach · Locations", component: LocationsPage },
  { path: "history", title: "Coach · History", component: HistoryPage },
  { path: "settings", title: "Coach · Settings", component: SettingsPage },
  { path: "", pathMatch: "full", redirectTo: "today" },
  { path: "**", redirectTo: "today" },
];
