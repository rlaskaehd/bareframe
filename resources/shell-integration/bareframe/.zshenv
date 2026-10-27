# bareframe's zsh startup (ZDOTDIR points here). Ghostty's integration runs first: it
# restores the user's ZDOTDIR, reads the user's .zshenv and installs its own hooks; the
# user's .zprofile and .zshrc then load from their own ZDOTDIR as usual.
'builtin' 'source' '--' "$BAREFRAME_GHOSTTY_ZSH/.zshenv"
'builtin' 'unset' 'BAREFRAME_GHOSTTY_ZSH'

# As in Warp, the prompt starts at the bottom of the screen and output scrolls up from
# there. Blank lines, not a cursor jump, take it down at startup and after a clear: a
# terminal starts before its pane has its final size, and when it grows Ghostty keeps the
# prompt at the bottom only by pulling rows down from history, so history must hold
# enough of them for the tallest window. Before each other prompt the cursor just drops
# to the last row. Ctrl+L (which Cmd+K also sends) clears the same way.
if [[ -o 'interactive' ]]; then
    _bareframe_pad() { 'builtin' 'print' -n -- ${(pl:200::\n:)} }
    _bareframe_bottom() {
        if [[ -n $_bareframe_cleared ]]; then
            _bareframe_cleared=
            _bareframe_pad
        else
            'builtin' 'print' -n -- $'\e[999B'
        fi
    }
    _bareframe_preexec() { [[ $1 == (clear|reset)(| *) ]] && _bareframe_cleared=1 }
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
