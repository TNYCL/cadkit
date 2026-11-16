// Reads the file written by scripts/prebuild.d/30-stats.mjs. A missing file (prebuild not run)
// is treated like an unreachable stats repository: the page renders the unavailable state.
import { buildModel } from './model.mjs';
import { BUILD_TIME } from '../config';

const files = import.meta.glob<{ default: unknown }>('./generated/stats.json', { eager: true });
const raw = Object.values(files)[0]?.default as
	| { status: 'ok'; checked_at: string; source: string; summary: unknown }
	| { status: 'unavailable'; checked_at: string; source: string; reason: string }
	| undefined;

export type StatsModel = ReturnType<typeof buildModel>;

export const stats:
	| { ok: true; model: StatsModel; checkedAt: string }
	| { ok: false; reason: string; checkedAt: string | null } =
	raw?.status === 'ok'
		? { ok: true, model: buildModel(raw.summary, BUILD_TIME), checkedAt: raw.checked_at }
		: { ok: false, reason: raw?.status === 'unavailable' ? raw.reason : 'statistics were not fetched for this build', checkedAt: raw?.checked_at ?? null };
