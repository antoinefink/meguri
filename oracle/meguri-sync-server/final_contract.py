# SPDX-License-Identifier: AGPL-3.0-or-later
"""Fixed Desktop31 selection and prospectively reviewed actual action inputs."""
import copy
import pathlib

from validation import decode, encoded, require, same_value, sha

RECIPE_SHA256 = 'f8f8488500dc11c8d3ce0da7185d0de280160b240da38a0cba11b94457902348'
NEUTRAL = pathlib.Path('/opt/reference/neutral')


def recipes():
    data = (NEUTRAL / 'final-fixture-recipes.json').read_bytes()
    require(sha(data) == RECIPE_SHA256, 'final fixture recipe changed')
    return decode(data)


def selection(previous):
    additions = decode((NEUTRAL / 'candidate-final-additions.json').read_bytes())
    require([len(case.get('actions',[])) for case in additions.get('cases',[])] == [4,5,5,2,1,6],'final action order contract is stale')
    require(additions.get('fixture_recipes_sha256') == RECIPE_SHA256 and len(additions.get('cases',[])) == 6, 'final additions changed')
    cases = []
    for phase,count in [('full',11),('normal',10),('media',4)]:
        old=previous(phase)
        frozen=decode((NEUTRAL / ('candidate-'+phase+'-selection.json')).read_bytes())
        require(same_value(old,frozen) and len(old['cases']) == count, 'historical candidate selection changed')
        cases += copy.deepcopy(old['cases'])
    cases += copy.deepcopy(additions['cases'])
    require(len(cases) == 31 and len({case['id'] for case in cases}) == 31, 'final Desktop selection count mismatch')
    recipes()
    return {'format':'meguri-sync-selection-v1','lane_kind':'candidate','phase':'final',
            'scope':'Exact final Docker package Desktop31; separate Mobile, retained-peer and broad research accounting.',
            'cases':cases}


SEGMENTS=('full','normal','media','migration','replacement-upload','replacement-download','metadata-only','fresh-fresh','rich')
SLICES={'full':(0,11),'normal':(11,21),'media':(21,25),'migration':(25,26),'replacement-upload':(26,27),
        'replacement-download':(27,28),'metadata-only':(28,29),'fresh-fresh':(29,30),'rich':(30,31)}


def segment_selection(phase,previous):
    segment=phase.removeprefix('final-');require(segment in SEGMENTS,'unknown fixed final segment')
    complete=selection(previous);start,end=SLICES[segment]
    return {**complete,'phase':phase,'cases':complete['cases'][start:end]}
