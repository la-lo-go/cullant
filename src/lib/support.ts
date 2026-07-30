/**
 * Ways to help the project, as data.
 *
 * Cullant is free, has no ads, needs no account and sends nothing anywhere, so
 * the only thing keeping it going is people choosing to help. The two lists
 * below feed the support dialog.
 *
 * NOTE ON THE URLS: none of the funding accounts exist yet, so every paid
 * channel points at FUNDING.md, which is the one place that says what is
 * actually live. As each account opens, its `url` here becomes the real one and
 * nothing else has to change. The repository itself is not published yet either,
 * so all of these 404 for now, deliberately.
 */

import Star from "@lucide/svelte/icons/star";
import Bug from "@lucide/svelte/icons/bug";
import Aperture from "@lucide/svelte/icons/aperture";
import Heart from "@lucide/svelte/icons/heart";
import Coffee from "@lucide/svelte/icons/coffee";
import CreditCard from "@lucide/svelte/icons/credit-card";

export const REPO_URL = "https://github.com/la-lo-go/cullant";
export const FUNDING_URL = `${REPO_URL}/blob/main/FUNDING.md`;

export interface SupportChannel {
  id: string;
  label: string;
  /** One short line on what it does for the project, or what to send. */
  note: string;
  url: string;
  icon: typeof Star;
}

/** Help that costs nothing. Listed first, and deliberately so: asking only for
 *  money reads worse than asking for help and mentioning money. */
export const FREE_WAYS: SupportChannel[] = [
  {
    id: "star",
    label: "Star the repository",
    note: "The cheapest way to help other photographers find it",
    url: REPO_URL,
    icon: Star,
  },
  {
    id: "bug",
    label: "Report a bug or an idea",
    note: "A precise report is worth more than an afternoon of guessing",
    url: `${REPO_URL}/issues/new`,
    icon: Bug,
  },
  {
    id: "raws",
    label: "Send RAWs from your camera",
    note: "Every body decodes differently, and testing needs files I do not own",
    url: `${REPO_URL}/issues/new?labels=raw-samples`,
    icon: Aperture,
  },
];

export const MONEY_WAYS: SupportChannel[] = [
  {
    id: "sponsors",
    label: "GitHub Sponsors",
    note: "Monthly or one-off, on the same account as the code",
    url: FUNDING_URL,
    icon: Heart,
  },
  {
    id: "kofi",
    label: "Ko-fi",
    note: "A one-off tip, no account needed",
    url: FUNDING_URL,
    icon: Coffee,
  },
  {
    id: "paypal",
    label: "PayPal",
    note: "If the other two are not your thing",
    url: FUNDING_URL,
    icon: CreditCard,
  },
];
