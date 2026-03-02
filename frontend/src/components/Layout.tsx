import { useState } from "react";
import { Outlet, Link, useLocation } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { useAuth } from "@/contexts/AuthContext";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import { LanguageSwitcher } from "@/components/LanguageSwitcher";
import { Cat, Home, LogOut, Settings, Menu, X } from "lucide-react";

export function Layout() {
  const { t } = useTranslation();
  const { user, logout } = useAuth();
  const location = useLocation();
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false);

  return (
    <div className="flex min-h-screen flex-col">
      <header className="sticky top-0 z-50 border-b bg-background/95 backdrop-blur supports-backdrop-filter:bg-background/60">
        <div className="container flex h-14 items-center">
          <Link to="/" className="flex items-center gap-2 font-semibold">
            <Cat className="h-6 w-6 text-primary" />
            <span className="hidden xs:inline">Home Monitor</span>
          </Link>

          <nav className="ml-6 hidden items-center gap-4 md:flex">
            <Link to="/">
              <Button variant={location.pathname === "/" ? "secondary" : "ghost"} size="sm">
                <Home className="mr-2 h-4 w-4" />
                {t("layout.dashboard")}
              </Button>
            </Link>
          </nav>

          <div className="ml-auto hidden items-center gap-4 md:flex">
            <LanguageSwitcher />
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <Settings className="h-4 w-4" />
              <span>{user?.username}</span>
              <span className="rounded bg-muted px-1.5 py-0.5 text-xs">{user?.role}</span>
            </div>
            <Separator orientation="vertical" className="h-6" />
            <Button variant="ghost" size="sm" onClick={logout}>
              <LogOut className="mr-2 h-4 w-4" />
              {t("auth.logout")}
            </Button>
          </div>

          <div className="ml-auto flex items-center gap-2 md:hidden">
            <LanguageSwitcher />
            <Button
              variant="ghost"
              size="icon"
              onClick={() => setMobileMenuOpen(!mobileMenuOpen)}
              aria-label="Toggle menu"
            >
              {mobileMenuOpen ? <X className="h-5 w-5" /> : <Menu className="h-5 w-5" />}
            </Button>
          </div>
        </div>

        {mobileMenuOpen && (
          <div className="border-t bg-background md:hidden">
            <div className="container py-4 space-y-4">
              <Link to="/" onClick={() => setMobileMenuOpen(false)}>
                <Button
                  variant={location.pathname === "/" ? "secondary" : "ghost"}
                  size="sm"
                  className="w-full justify-start"
                >
                  <Home className="mr-2 h-4 w-4" />
                  {t("layout.dashboard")}
                </Button>
              </Link>

              <Separator />

              <div className="flex items-center gap-2 text-sm text-muted-foreground px-3">
                <Settings className="h-4 w-4" />
                <span>{user?.username}</span>
                <span className="rounded bg-muted px-1.5 py-0.5 text-xs">{user?.role}</span>
              </div>

              <Button
                variant="ghost"
                size="sm"
                onClick={() => {
                  logout();
                  setMobileMenuOpen(false);
                }}
                className="w-full justify-start text-destructive hover:text-destructive"
              >
                <LogOut className="mr-2 h-4 w-4" />
                {t("auth.logout")}
              </Button>
            </div>
          </div>
        )}
      </header>

      <main className="flex-1 container py-6">
        <Outlet />
      </main>

      <footer className="border-t py-4">
        <div className="container text-center text-sm text-muted-foreground">
          {t("layout.footer", { year: new Date().getFullYear() })}
        </div>
      </footer>
    </div>
  );
}
