import { Component, computed, inject, signal } from "@angular/core";
import { MatProgressBarModule } from "@angular/material/progress-bar";

import { CoachApi } from "../../coach-api";
import { GroupBalance, Region } from "../../models";

const REGION_ORDER: Region[] = ["chest", "back", "shoulders", "arms", "forearms", "core", "legs"];

/** The muscle-group volume picture the coach reasons over — rolling volume vs
 *  target per group, by region. Shows the user their own data. */
@Component({
  selector: "app-balance",
  templateUrl: "./balance.html",
  styleUrl: "./balance.scss",
  imports: [MatProgressBarModule],
})
export class BalancePage {
  private api = inject(CoachApi);

  readonly groups = signal<GroupBalance[]>([]);
  readonly loading = signal(true);

  readonly byRegion = computed(() => {
    const by = new Map<Region, GroupBalance[]>();
    for (const g of this.groups()) {
      const arr = by.get(g.region) ?? [];
      arr.push(g);
      by.set(g.region, arr);
    }
    return REGION_ORDER.filter((r) => by.has(r)).map((r) => ({
      region: r,
      groups: by.get(r)!.sort((a, b) => b.deficit - a.deficit),
    }));
  });

  constructor() {
    this.api.pacingNow().subscribe({
      next: (p) => {
        this.groups.set(p.groups);
        this.loading.set(false);
      },
      error: () => this.loading.set(false),
    });
  }

  regionLabel(r: string): string {
    return r.charAt(0).toUpperCase() + r.slice(1);
  }
  pct(current: number, target: number): number {
    return target > 0 ? Math.min(100, Math.round((current / target) * 100)) : 0;
  }
  round1(n: number): string {
    return (Math.round(n * 10) / 10).toString();
  }
  round0(n: number): string {
    return Math.round(n).toString();
  }
}
