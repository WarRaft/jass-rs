// @dialect vjass
globals
    integer gCount = 0
endglobals

//! zinc
library Counter {
    /* counts up */
    public function Bump(integer by) -> integer {
        gCount += by;
        return gCount;
    }
}
//! endzinc

function Main takes nothing returns nothing
    call Bump(1)
endfunction
