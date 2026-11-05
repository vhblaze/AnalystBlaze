import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

/**
 * Botão D3 (doc 15 §4).
 * - `primary`: um por tela, a ação que resolve o problema do card.
 * - `outline`: todo o resto.
 * - `danger`: só o que não tem volta, e sempre depois de <ConfirmInline/>.
 * - `link`: navegação ("Meu PC →", "Histórico completo").
 */
const buttonVariants = cva(
  "inline-flex shrink-0 items-center justify-center gap-2 whitespace-nowrap rounded-control font-medium transition-colors duration-fast ease-out focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/60 focus-visible:ring-offset-2 focus-visible:ring-offset-bg disabled:cursor-not-allowed disabled:opacity-50 [&_svg]:size-4 [&_svg]:shrink-0",
  {
    variants: {
      variant: {
        primary: "bg-accent font-semibold text-accent-ink hover:bg-accent/90",
        outline: "border border-line bg-transparent text-fg hover:border-fg-4/60 hover:bg-surface-2",
        danger: "border border-danger-line bg-transparent text-danger hover:bg-danger/10",
        link: "rounded-sm text-accent-2 underline underline-offset-4 hover:text-accent",
      },
      size: {
        md: "h-9 px-4 text-sm",
        sm: "h-8 px-3 text-[13px]",
      },
    },
    compoundVariants: [{ variant: "link", className: "h-auto px-0" }],
    defaultVariants: { variant: "outline", size: "md" },
  },
);

export type ButtonProps = React.ButtonHTMLAttributes<HTMLButtonElement> &
  VariantProps<typeof buttonVariants> & {
    /** Desabilita e explica por quê (vem de useHelper/usePlan, nunca inventado pela tela). */
    disabledReason?: string | null;
  };

export const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, disabled, disabledReason, title, type = "button", ...props }, ref) => (
    <button
      ref={ref}
      type={type}
      disabled={disabled || Boolean(disabledReason)}
      title={disabledReason ?? title}
      className={cn(buttonVariants({ variant, size }), className)}
      {...props}
    />
  ),
);
Button.displayName = "Button";

export { buttonVariants };
