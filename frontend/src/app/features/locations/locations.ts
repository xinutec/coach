import { Component, computed, inject, signal } from "@angular/core";
import { FormsModule } from "@angular/forms";
import { MatButtonModule } from "@angular/material/button";
import { MatFormFieldModule } from "@angular/material/form-field";
import { MatIconModule } from "@angular/material/icon";
import { MatInputModule } from "@angular/material/input";
import { MatSlideToggleModule } from "@angular/material/slide-toggle";
import { forkJoin } from "rxjs";

import { CoachApi } from "../../coach-api";
import { Category, Equipment, Location } from "../../models";

const CATEGORY_LABEL: Record<Category, string> = {
  free_weight: "Free weights",
  band: "Bands",
  machine: "Machines",
  ball: "Balls",
  rig: "Bars & rings",
  bench: "Bench",
};
const CATEGORY_ORDER: Category[] = ["free_weight", "rig", "bench", "machine", "band", "ball"];

/** Manage training locations: each is a named place with an equipment inventory.
 *  The Today view uses the selected location to decide what to do there. */
@Component({
  selector: "app-locations",
  templateUrl: "./locations.html",
  styleUrl: "./locations.scss",
  imports: [
    FormsModule,
    MatButtonModule,
    MatFormFieldModule,
    MatIconModule,
    MatInputModule,
    MatSlideToggleModule,
  ],
})
export class LocationsPage {
  private api = inject(CoachApi);

  readonly locations = signal<Location[]>([]);
  readonly equipment = signal<Equipment[]>([]);
  readonly loading = signal(true);

  // Form state: editingId is null (hidden), 0 (new), or an id (editing).
  readonly editingId = signal<number | null>(null);
  readonly formName = signal("");
  readonly formDefault = signal(false);
  readonly formEquip = signal<Set<string>>(new Set());
  readonly saving = signal(false);

  /** Equipment grouped by category, in a stable order, for the picker. */
  readonly grouped = computed(() => {
    const byCat = new Map<Category, Equipment[]>();
    for (const e of this.equipment()) {
      const arr = byCat.get(e.category) ?? [];
      arr.push(e);
      byCat.set(e.category, arr);
    }
    return CATEGORY_ORDER.filter((c) => byCat.has(c)).map((c) => ({
      category: c,
      label: CATEGORY_LABEL[c],
      items: byCat.get(c)!,
    }));
  });

  constructor() {
    this.reload();
  }

  reload(): void {
    this.loading.set(true);
    forkJoin({ locations: this.api.locations(), equipment: this.api.equipment() }).subscribe({
      next: ({ locations, equipment }) => {
        this.locations.set(locations);
        this.equipment.set(equipment);
        this.loading.set(false);
      },
      error: () => this.loading.set(false),
    });
  }

  private equipmentNames = computed(
    () => new Map(this.equipment().map((e) => [e.slug, e.name])),
  );
  equipLabel(slug: string): string {
    return this.equipmentNames().get(slug) ?? slug;
  }

  startNew(): void {
    this.editingId.set(0);
    this.formName.set("");
    this.formDefault.set(this.locations().length === 0);
    this.formEquip.set(new Set());
  }

  startEdit(loc: Location): void {
    this.editingId.set(loc.id);
    this.formName.set(loc.name);
    this.formDefault.set(loc.isDefault);
    this.formEquip.set(new Set(loc.equipment));
  }

  cancel(): void {
    this.editingId.set(null);
  }

  toggleEquip(slug: string): void {
    const s = new Set(this.formEquip());
    if (s.has(slug)) s.delete(slug);
    else s.add(slug);
    this.formEquip.set(s);
  }

  save(): void {
    const id = this.editingId();
    if (id === null) return;
    const body = {
      name: this.formName().trim() || "Location",
      isDefault: this.formDefault(),
      equipment: [...this.formEquip()],
    };
    this.saving.set(true);
    const done = {
      next: () => {
        this.saving.set(false);
        this.editingId.set(null);
        this.reload();
      },
      error: () => this.saving.set(false),
    };
    if (id === 0) this.api.createLocation(body).subscribe(done);
    else this.api.patchLocation(id, body).subscribe(done);
  }

  remove(loc: Location): void {
    this.api.deleteLocation(loc.id).subscribe(() => this.reload());
  }
}
