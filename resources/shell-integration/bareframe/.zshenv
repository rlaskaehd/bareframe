# bareframe's zsh startup (ZDOTDIR points here). Ghostty's integration runs first: it
# restores the user's ZDOTDIR, reads the user's .zshenv and installs its own hooks; the
# user's .zprofile and .zshrc then load from their own ZDOTDIR as usual.
'builtin' 'source' '--' "$BAREFRAME_GHOSTTY_ZSH/.zshenv"
'builtin' 'unset' 'BAREFRAME_GHOSTTY_ZSH'
_bareframe_prompt=$BAREFRAME_PROMPT
'builtin' 'unset' 'BAREFRAME_PROMPT'

# With Settings' prompt position at Bottom (BAREFRAME_PROMPT=bottom), as in Warp, the
# prompt starts at the bottom of the screen and output scrolls up from there. Blank lines,
# not a cursor jump, take it down at startup and after a clear: a terminal starts before
# its pane has its final size, and when it grows Ghostty keeps the prompt at the bottom
# only by pulling rows down from history, so history must hold enough of them for the
# tallest window. Before each other prompt the cursor just drops
# to the last row. Ctrl+L (which Cmd+K also sends) clears the same way. Otherwise the
# prompt starts at the top, as in other terminals.
if [[ -o 'interactive' && $_bareframe_prompt == bottom ]]; then
    _bareframe_pad() { 'builtin' 'print' -n -- ${(pl:200::\n:)} }
    _bareframe_bottom() {
        if [[ -n $_bareframe_cleared ]]; then
            _bareframe_cleared=
            _bareframe_pad
        else
            'builtin' 'print' -n -- $'\e[999B'
        fi
    }
    # `clear` or `reset` wipes the history the blank lines live in, typed as is, through an
    # alias (zsh hands the expanded line as $3) or in a list such as `clear && ls`.
    _bareframe_preexec() {
        'builtin' 'local' word first=1
        for word in ${(z)3}; do
            (( first )) && [[ $word == (clear|reset) ]] && _bareframe_cleared=1
            [[ $word == (';'|'&&'|'||'|'|'|'&') ]] && first=1 || first=0
        done
    }
    _bareframe_clear_screen() {
        'builtin' 'zle' -I
        'builtin' 'print' -n -- $'\e[H\e[2J'
        _bareframe_pad
    }
    _bareframe_install() {
        'builtin' 'zle' -N clear-screen _bareframe_clear_screen
        precmd_functions=(${precmd_functions:#_bareframe_install})
    }
    'builtin' 'typeset' -ga precmd_functions preexec_functions
    precmd_functions=(_bareframe_bottom _bareframe_install $precmd_functions)
    preexec_functions=(_bareframe_preexec $preexec_functions)
    # Before the user's startup files, so anything they print shows above the prompt.
    _bareframe_pad
fi
