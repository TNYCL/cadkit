// RFC 9116 security.txt. Expires is the build date plus 180 days, so every deploy refreshes it.
import type { APIRoute } from 'astro';
import { SITE, EMAIL, LINKS, BUILD_TIME } from '../../config';

const DAY = 24 * 60 * 60 * 1000;

export const GET: APIRoute = () => {
	const expires = new Date(BUILD_TIME.getTime() + 180 * DAY);
	expires.setUTCHours(0, 0, 0, 0);
	const body = [
		`Contact: mailto:${EMAIL.security}`,
		`Contact: ${LINKS.securityAdvisory}`,
		`Expires: ${expires.toISOString().replace('.000Z', 'Z')}`,
		`Canonical: ${SITE}/.well-known/security.txt`,
		`Policy: ${SITE}/security/`,
		'Preferred-Languages: en',
		'',
	].join('\n');
	return new Response(body, { headers: { 'Content-Type': 'text/plain; charset=utf-8' } });
};
