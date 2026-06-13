// Locale link type — used by SidebarTree/SidebarToggle footer locale switcher.
// CCResDoc is single-locale (EN-only), so locale links are never populated,
// but the component API still types them.

export interface LocaleLink {
  code: string;
  label: string;
  href: string;
  active: boolean;
}
