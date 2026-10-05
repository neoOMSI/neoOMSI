import { useEffect, useRef } from 'react';
import mark from '../../assets/logos/icon-gradient.svg?trim';
import wordmark from '../../assets/logos/wordmark-gradient-dark.svg?trim';
import wordmarkLight from '../../assets/logos/wordmark-gradient-light.svg?trim';
import { DISCORD, DOCS, REPO } from './content/data';
import { useRoute } from './lib/hooks';
import { Icon } from './components/icons';
import { Docs } from './pages/Docs';
import { Download } from './pages/Download';
import { Faq } from './pages/Faq';
import { Home } from './pages/Home';
import { Issue, Issues } from './pages/Issues';
import { NotFound } from './pages/NotFound';
import { OpenOmsi } from './pages/OpenOmsi';
import { Releases } from './pages/Releases';
import { BASE, docPath, navigate, url, type Route } from './lib/routes';
import { headTags, meta } from './lib/seo';
import { useTheme } from './lib/theme';

const NAV = [
	['Docs', url(docPath('USER_GUIDE')), '/docs'],
	['FAQ', url('/faq/'), '/faq'],
	['Releases', url('/releases/'), '/releases'],
	['Issues', url('/issues/'), '/issues'],
	['GitHub', `https://github.com/${REPO}`],
	['Discord', DISCORD]
];

function Header({ path, wide }: { path: string; wide: boolean }) {
	const { theme, toggle } = useTheme();
	const light = theme === 'light';
	return (
		<header className="fixed inset-x-0 top-0 z-50 bg-page">
			<div
				className={`wrap flex flex-wrap items-center gap-x-8 py-3 sm:h-16 sm:py-0${wide ? ' max-w-[1440px] lg:px-10' : ''}`}
			>
				<a
					href={url('/')}
					className="flex shrink-0 items-center"
					aria-label="neoOMSI home"
				>
					<img className="-my-2 -ml-1.5 size-10" src={mark} alt="" />
					<img
						className="logo-dark ml-1 h-4 w-auto"
						src={wordmark}
						alt="neoOMSI"
					/>
					<img
						className="logo-light ml-1 h-4 w-auto"
						src={wordmarkLight}
						alt="neoOMSI"
					/>
				</a>
				<div className="ml-auto flex items-center gap-2 sm:order-last sm:ml-0">
					<button
						type="button"
						className="theme-toggle"
						onClick={toggle}
						aria-label={
							light
								? 'Switch to dark mode'
								: 'Switch to light mode'
						}
					>
						<Icon
							name={light ? 'dark_mode' : 'light_mode'}
							size={20}
						/>
					</button>
					<a
						className="btn gap-1.5 px-4 py-1.5 text-[15px]"
						href={url('/download/')}
					>
						<Icon name="download" size={18} />
						Download
					</a>
				</div>
				<nav
					id="nav"
					aria-label="Main"
					className="-mx-4 flex w-full gap-6 overflow-x-auto px-4 pt-2 font-medium sm:mx-0 sm:ml-auto sm:w-auto sm:px-0 sm:pt-0"
				>
					{NAV.map(([label, href, route]) => (
						<a
							key={label}
							href={href}
							className={
								route &&
								(path === route || path.startsWith(route + '/'))
									? 'active'
									: undefined
							}
						>
							{label}
						</a>
					))}
				</nav>
			</div>
		</header>
	);
}

const Footer = () => (
	<footer>
		<div className="wrap flex max-w-[max(70vw,1360px)] flex-wrap items-start justify-between gap-x-12 gap-y-4 py-10 text-[15px] text-muted xl:flex-nowrap">
			<p className="max-w-[38em] xl:max-w-none xl:flex-[0_1_44em]">
				neoOMSI is an independent project, not affiliated with the
				makers of OMSI&nbsp;2. OMSI&nbsp;2 is required to play. neoOMSI
				is licensed under{' '}
				<a
					className="link"
					href={`https://github.com/${REPO}/blob/main/LICENSE`}
				>
					GPL-3.0-or-later
				</a>
				.
			</p>
			<nav className="flex min-w-0 flex-wrap gap-x-6 gap-y-1 xl:flex-[0_1_auto]">
				<a
					className="hover:text-ink"
					href={`https://github.com/${REPO}`}
				>
					Source code
				</a>
				<a className="hover:text-ink" href={url(docPath('BUILDING'))}>
					Building
				</a>
				<a
					className="hover:text-ink"
					href={url(docPath('DEVELOPMENT'))}
				>
					Contributing
				</a>
				<a className="hover:text-ink" href={url('/faq/')}>
					FAQ
				</a>
				<a className="hover:text-ink" href={url('/openomsi/')}>
					neoOMSI vs openOMSI
				</a>
				<a className="hover:text-ink" href={DISCORD}>
					Discord
				</a>
			</nav>
			<p className="xl:shrink-0 xl:text-right">
				<span className="block">End of the line, please alight.</span>
				<span className="block xl:whitespace-nowrap">
					Built by{' '}
					<a
						className="font-semibold text-ink hover:text-accent"
						href="https://devjakob.com"
					>
						devjakob.com
					</a>
					, four minutes late as usual.
				</span>
			</p>
		</div>
	</footer>
);

function Page({ path }: { path: string }) {
	const doc = DOCS.find((d) => path === `/docs/${d.slug}`);
	if (doc) return <Docs file={doc.file} />;
	if (path === '/') return <Home />;
	if (path === '/download') return <Download />;
	if (path === '/releases') return <Releases />;
	if (path === '/issues') return <Issues />;
	if (path === '/faq') return <Faq />;
	if (path === '/openomsi') return <OpenOmsi />;
	const issue = path.match(/^\/issues\/(\d+)$/)?.[1];
	if (issue) return <Issue key={issue} n={issue} />;
	return <NotFound />;
}

function follow(e: MouseEvent) {
	if (
		e.defaultPrevented ||
		e.button !== 0 ||
		e.metaKey ||
		e.ctrlKey ||
		e.shiftKey ||
		e.altKey
	)
		return;
	const a = (e.target as Element).closest('a');
	if (!a || a.target || a.hasAttribute('download')) return;
	const to = new URL(a.href, location.href);
	if (to.origin !== location.origin || !to.pathname.startsWith(BASE)) return;
	e.preventDefault();
	navigate(to.pathname + to.hash);
}

function applyHead(path: string) {
	document.head.querySelectorAll('[data-head]').forEach((e) => e.remove());
	const t = document.createElement('template');
	t.innerHTML = headTags(meta(path));
	document.head.append(t.content);
}

export function App({ route }: { route?: Route }) {
	const { path, anchor } = useRoute(route);
	const docs = path.startsWith('/docs/');
	const first = useRef(true);

	useEffect(() => {
		document.addEventListener('click', follow);
		return () => document.removeEventListener('click', follow);
	}, []);

	useEffect(() => applyHead(path), [path]);

	useEffect(() => {
		const target = anchor && document.getElementById(anchor);
		if (target) target.scrollIntoView();
		else if (!first.current) window.scrollTo(0, 0);
		first.current = false;
	}, [path, anchor]);

	return (
		<>
			<Header path={path} wide={docs} />
			<main className="min-h-[60vh]">
				<Page path={path} />
			</main>
			<Footer />
		</>
	);
}
