#!/usr/bin/env python3
"""Puts Anki's own translations into the Flashcards plugin's i18n files where Anki has the term.

usage: <matcha-anki-server venv python> tools/anki_terms.py plugins/flashcards/i18n
(needs the `anki` package, as matcha-anki-server installs it)
"""
import json, os, sys
import anki.lang as L

CODES = {'DE': 'de_DE', 'FR': 'fr_FR', 'ES': 'es_ES', 'IT': 'it_IT', 'PT': 'pt_BR', 'P2': 'pt_PT', 'CA': 'ca_ES',
         'CAV': 'ca_ES', 'NL': 'nl_NL', 'DA': 'da_DK', 'NB': 'nb_NO', 'SV': 'sv_SE', 'FI': 'fi_FI', 'HU': 'hu_HU',
         'PL': 'pl_PL', 'CS': 'cs_CZ', 'SK': 'sk_SK', 'SI': 'sl_SI', 'RO': 'ro_RO', 'TR': 'tr_TR', 'RU': 'ru_RU',
         'UK': 'uk_UA', 'BE': 'be_BY', 'BG': 'bg_BG', 'VI': 'vi_VN', 'JA': 'ja_JP', 'ZHS': 'zh_CN', 'ZHT': 'zh_TW',
         'AR': 'ar_SA', 'HE': 'he_IL', 'FA': 'fa_IR'}
KEYS = {'again': 'studying_again', 'hard': 'studying_hard', 'good': 'studying_good', 'easy': 'studying_easy',
        'retention': 'deck_config_desired_retention', 'new_per_day': 'scheduling_new_cardsday',
        'max_interval': 'scheduling_maximum_interval'}
UNITS = {'interval_m': ('scheduling_answer_button_time_minutes', '%d'), 'interval_h': ('scheduling_answer_button_time_hours', '%d'),
         'interval_d': ('scheduling_answer_button_time_days', '%d'), 'interval_mo': ('scheduling_answer_button_time_months', '%d'),
         'interval_y': ('scheduling_answer_button_time_years', '%s')}


def get(name, **kw):
    return getattr(L.tr_legacyglobal, name)(**kw).replace('\u2068', '').replace('\u2069', '')


def main():
    folder = sys.argv[1]
    L.set_lang('en_US')
    en = {k: get(v) for k, v in KEYS.items()}
    en_units = {k: get(v, amount=7777) for k, (v, _) in UNITS.items()}
    for code, loc in CODES.items():
        L.set_lang(loc)
        path = os.path.join(folder, code + '.json')
        d = json.load(open(path))
        for k, v in KEYS.items():
            t = get(v)
            if t and t != en[k]:  # only where Anki translates it
                d[k] = t
        for k, (v, ph) in UNITS.items():
            t = get(v, amount=7777)
            if '7777' in t and t != en_units[k]:
                d[k] = t.replace('7777', ph)
        with open(path, 'w') as f:
            json.dump(d, f, ensure_ascii=False, indent=2)
            f.write('\n')


if __name__ == '__main__':
    main()
