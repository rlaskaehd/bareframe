# bareframe's zsh startup (ZDOTDIR points here). It runs Ghostty's integration, which
# restores the user's ZDOTDIR, reads the user's .zshenv and installs its own hooks; the
# user's .zprofile and .zshrc then load from their own ZDOTDIR as usual.
'builtin' 'source' '--' "$BAREFRAME_GHOSTTY_ZSH/.zshenv"
'builtin' 'unset' 'BAREFRAME_GHOSTTY_ZSH'
