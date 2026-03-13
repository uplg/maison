import { useState } from "react";
import { Navigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { useAuth } from "@/contexts/AuthContext";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { toast } from "@/hooks/use-toast";
import { LanguageSwitcher } from "@/components/LanguageSwitcher";
import { House, Loader2, LockKeyhole, Radio, ShieldCheck } from "lucide-react";

export function LoginPage() {
  const { t } = useTranslation();
  const { isAuthenticated, login } = useAuth();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [isLoading, setIsLoading] = useState(false);

  if (isAuthenticated) {
    return <Navigate to="/" replace />;
  }

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setIsLoading(true);

    try {
      await login(username, password);
      toast({
        title: t("auth.loginSuccess"),
        description: t("auth.loginSuccessDescription"),
      });
    } catch (error) {
      toast({
        title: t("auth.loginError"),
        description: error instanceof Error ? error.message : t("auth.invalidCredentials"),
        variant: "destructive",
      });
    } finally {
      setIsLoading(false);
    }
  };

  return (
    <div className="relative flex min-h-screen items-center justify-center overflow-hidden p-4">
      <div className="pointer-events-none absolute inset-0 bg-[radial-gradient(circle_at_top_left,hsl(193_77%_86%/.7),transparent_30%),radial-gradient(circle_at_85%_18%,hsl(36_100%_86%/.6),transparent_24%),linear-gradient(180deg,hsl(38_60%_98%),hsl(210_45%_98%))]" />
      <div className="absolute top-4 right-4">
        <LanguageSwitcher />
      </div>
      <div className="relative w-full max-w-5xl overflow-hidden rounded-[2rem] border border-white/70 bg-white/85 shadow-[0_32px_100px_-36px_rgba(15,23,42,0.35)] backdrop-blur">
        <div className="grid lg:grid-cols-[1.1fr_0.9fr]">
          <div className="border-b border-slate-200/80 p-8 lg:border-r lg:border-b-0 lg:p-12">
            <div className="mb-8 flex h-14 w-14 items-center justify-center rounded-2xl bg-slate-900 text-white shadow-lg shadow-slate-900/15">
              <House className="h-7 w-7" />
            </div>
            <div className="space-y-4">
              <p className="text-sm font-semibold uppercase tracking-[0.24em] text-slate-500">
                {t("branding.kicker")}
              </p>
              <h1 className="max-w-md text-4xl font-semibold tracking-[-0.04em] text-slate-950 sm:text-5xl">
                {t("branding.name")}
              </h1>
              <p className="max-w-xl text-base leading-7 text-slate-600 sm:text-lg">
                {t("auth.loginDescription")}
              </p>
            </div>
            <div className="mt-10 grid gap-4 sm:grid-cols-3">
              <div className="rounded-2xl border border-slate-200/80 bg-white/80 p-4">
                <ShieldCheck className="mb-3 h-5 w-5 text-slate-900" />
                <p className="text-sm font-medium text-slate-950">{t("auth.securityTitle")}</p>
                <p className="mt-1 text-sm text-slate-500">{t("auth.securityDescription")}</p>
              </div>
              <div className="rounded-2xl border border-slate-200/80 bg-white/80 p-4">
                <Radio className="mb-3 h-5 w-5 text-slate-900" />
                <p className="text-sm font-medium text-slate-950">{t("auth.localTitle")}</p>
                <p className="mt-1 text-sm text-slate-500">{t("auth.localDescription")}</p>
              </div>
              <div className="rounded-2xl border border-slate-200/80 bg-white/80 p-4">
                <LockKeyhole className="mb-3 h-5 w-5 text-slate-900" />
                <p className="text-sm font-medium text-slate-950">{t("auth.accessTitle")}</p>
                <p className="mt-1 text-sm text-slate-500">{t("auth.accessDescription")}</p>
              </div>
            </div>
          </div>

          <div className="p-6 sm:p-8 lg:p-12">
            <Card className="border-slate-200/80 bg-white shadow-none">
              <CardHeader className="space-y-2 text-left">
                <CardTitle className="text-2xl tracking-[-0.03em] text-slate-950">{t("auth.login")}</CardTitle>
                <CardDescription>{t("auth.loginCardDescription")}</CardDescription>
              </CardHeader>
              <CardContent>
                <form onSubmit={handleSubmit} className="space-y-4">
                  <div className="space-y-2">
                    <Label htmlFor="username">{t("auth.username")}</Label>
                    <Input
                      id="username"
                      type="text"
                      placeholder="admin"
                      value={username}
                      onChange={(e) => setUsername(e.target.value)}
                      required
                      autoComplete="username"
                      disabled={isLoading}
                      className="h-11 border-slate-200 bg-slate-50/70"
                    />
                  </div>
                  <div className="space-y-2">
                    <Label htmlFor="password">{t("auth.password")}</Label>
                    <Input
                      id="password"
                      type="password"
                      placeholder="••••••••"
                      value={password}
                      onChange={(e) => setPassword(e.target.value)}
                      required
                      autoComplete="current-password"
                      disabled={isLoading}
                      className="h-11 border-slate-200 bg-slate-50/70"
                    />
                  </div>
                  <Button type="submit" className="h-11 w-full bg-slate-950 text-white hover:bg-slate-800" disabled={isLoading}>
                    {isLoading ? (
                      <>
                        <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                        {t("auth.loggingIn")}
                      </>
                    ) : (
                      t("auth.login")
                    )}
                  </Button>
                </form>
              </CardContent>
            </Card>
          </div>
        </div>
      </div>
    </div>
  );
}
